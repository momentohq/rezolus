const ONLINE_CORES_REFRESH: Duration = Duration::from_secs(1);

mod bpf {
    include!(concat!(env!("OUT_DIR"), "/cpu_usage.bpf.rs"));
}

use super::NAME;

use std::io::{Read, Seek};

use metriken::MetricBuilder;

use bpf::*;

use crate::common::bpf::*;
use crate::common::*;
use crate::samplers::cpu::stats::*;
use crate::samplers::cpu::*;
use crate::samplers::hwinfo::hardware_info;

impl GetMap for ModSkel<'_> {
    fn map(&self, name: &str) -> &libbpf_rs::Map {
        self.obj.map(name).unwrap()
    }
}

/// Collects CPU Usage stats using BPF and traces:
/// * __cgroup_account_cputime_field
///
/// And produces these stats:
/// * cpu/usage/*

pub struct CpuUsage {
    bpf: Bpf<ModSkel<'static>>,
    percpu_counters: Arc<PercpuCounters>,
    sum_prev: u64,
    percpu_sum_prev: Vec<u64>,
    counter_interval: Duration,
    counter_next: Instant,
    counter_prev: Instant,
    distribution_interval: Duration,
    distribution_next: Instant,
    distribution_prev: Instant,
    online_cores: usize,
    online_cores_file: std::fs::File,
    online_cores_interval: Duration,
    online_cores_next: Instant,
}

impl CpuUsage {
    pub fn new(config: &Config) -> Result<Self, ()> {
        let builder = ModSkelBuilder::default();
        let mut skel = builder
            .open()
            .map_err(|e| error!("failed to open bpf builder: {e}"))?
            .load()
            .map_err(|e| error!("failed to load bpf program: {e}"))?;

        skel.attach()
            .map_err(|e| error!("failed to attach bpf program: {e}"))?;

        let mut bpf = Bpf::from_skel(skel);

        let mut online_cores_file = std::fs::File::open("/sys/devices/system/cpu/online")
            .map_err(|e| error!("couldn't open: {e}"))?;

        let online_cores = online_cores(&mut online_cores_file)
            .map_err(|_| error!("couldn't determine number of online cores"))?;

        let cpus = match hardware_info() {
            Ok(hwinfo) => hwinfo.get_cpus(),
            Err(_) => return Err(()),
        };

        let counters = vec![
            Counter::new(&CPU_USAGE_USER, Some(&CPU_USAGE_USER_HISTOGRAM)),
            Counter::new(&CPU_USAGE_NICE, Some(&CPU_USAGE_NICE_HISTOGRAM)),
            Counter::new(&CPU_USAGE_SYSTEM, Some(&CPU_USAGE_SYSTEM_HISTOGRAM)),
            Counter::new(&CPU_USAGE_IDLE, Some(&CPU_USAGE_IDLE_HISTOGRAM)),
            Counter::new(&CPU_USAGE_IO_WAIT, Some(&CPU_USAGE_IO_WAIT_HISTOGRAM)),
            Counter::new(&CPU_USAGE_IRQ, Some(&CPU_USAGE_IRQ_HISTOGRAM)),
            Counter::new(&CPU_USAGE_SOFTIRQ, Some(&CPU_USAGE_SOFTIRQ_HISTOGRAM)),
            Counter::new(&CPU_USAGE_STEAL, Some(&CPU_USAGE_STEAL_HISTOGRAM)),
            Counter::new(&CPU_USAGE_GUEST, Some(&CPU_USAGE_GUEST_HISTOGRAM)),
            Counter::new(&CPU_USAGE_GUEST_NICE, Some(&CPU_USAGE_GUEST_NICE_HISTOGRAM)),
        ];

        let mut percpu_counters = PercpuCounters::default();

        let states = [
            "user",
            "nice",
            "system",
            "idle",
            "io_wait",
            "irq",
            "softirq",
            "steal",
            "guest",
            "guest_nice",
        ];

        for cpu in cpus {
            for state in states {
                percpu_counters.push(
                    cpu.id(),
                    MetricBuilder::new("cpu/usage")
                        .metadata("id", format!("{}", cpu.id()))
                        .metadata("core", format!("{}", cpu.core()))
                        .metadata("die", format!("{}", cpu.die()))
                        .metadata("package", format!("{}", cpu.package()))
                        .metadata("state", state)
                        .formatter(cpu_metric_formatter)
                        .build(metriken::Counter::new()),
                );
            }
        }

        let percpu_counters = Arc::new(percpu_counters);

        bpf.add_counters_with_percpu("counters", counters, percpu_counters.clone());

        let mut distributions = vec![];

        for (name, histogram) in distributions.drain(..) {
            bpf.add_distribution(name, histogram);
        }

        let now = Instant::now();

        Ok(Self {
            bpf,
            percpu_counters,
            sum_prev: 0,
            percpu_sum_prev: vec![0; cpus.len()],
            counter_interval: config.interval(NAME),
            counter_next: now,
            counter_prev: now,
            distribution_interval: config.distribution_interval(NAME),
            distribution_next: now,
            distribution_prev: now,
            online_cores,
            online_cores_file,
            online_cores_interval: ONLINE_CORES_REFRESH,
            online_cores_next: now + ONLINE_CORES_REFRESH,
        })
    }

    pub fn refresh_counters(&mut self, now: Instant) {
        if now < self.counter_next {
            return;
        }

        // get the amount of time since we last sampled
        let elapsed = now - self.counter_prev;

        // refresh the counters from the kernel-space counters
        self.bpf.refresh_counters(elapsed.as_secs_f64());

        // get the new sum of all the counters
        let sum_now: u64 = sum();

        // get the number of nanoseconds in busy time, since idle hasn't been
        // incremented, the busy time is the difference between our prev and
        // current sums
        let busy_delta = sum_now.wrapping_sub(self.sum_prev);

        // idle time delta is `cores * elapsed - busy_delta`
        let idle_delta = self.online_cores as u64 * elapsed.as_nanos() as u64 - busy_delta;

        // update the idle time metrics
        CPU_USAGE_IDLE.add(idle_delta);
        let _ = CPU_USAGE_IDLE_HISTOGRAM.increment(idle_delta);

        // do the same for percpu counters
        for (cpu, sum_prev) in self.percpu_sum_prev.iter_mut().enumerate() {
            let sum_now: u64 = self.percpu_counters.sum(cpu).unwrap_or(0);
            let busy_delta = sum_now.wrapping_sub(*sum_prev);
            let idle_delta = elapsed.as_nanos() as u64 - busy_delta;
            self.percpu_counters.add(cpu, 3, idle_delta);
            *sum_prev += busy_delta + idle_delta;
        }

        // update the previous sums
        self.sum_prev += busy_delta + idle_delta;

        // determine when to sample next
        let next = self.counter_next + self.counter_interval;

        // check that next sample time is in the future
        if next > now {
            self.counter_next = next;
        } else {
            self.counter_next = now + self.counter_interval;
        }

        // mark when we last sampled
        self.counter_prev = now;
    }

    pub fn refresh_distributions(&mut self, now: Instant) {
        if now < self.distribution_next {
            return;
        }

        self.bpf.refresh_distributions();

        // determine when to sample next
        let next = self.distribution_next + self.distribution_interval;

        // check that next sample time is in the future
        if next > now {
            self.distribution_next = next;
        } else {
            self.distribution_next = now + self.distribution_interval;
        }

        // mark when we last sampled
        self.distribution_prev = now;
    }

    pub fn update_online_cores(&mut self, now: Instant) {
        if now < self.online_cores_next {
            return;
        }

        if let Ok(v) = online_cores(&mut self.online_cores_file) {
            self.online_cores = v;
        }

        // determine when to update next
        let next = self.online_cores_next + self.online_cores_interval;

        // check that next update time is in the future
        if next > now {
            self.online_cores_next = next;
        } else {
            self.online_cores_next = now + self.online_cores_interval;
        }
    }
}

fn sum() -> u64 {
    [
        &CPU_USAGE_USER,
        &CPU_USAGE_NICE,
        &CPU_USAGE_SYSTEM,
        &CPU_USAGE_IDLE,
        &CPU_USAGE_IO_WAIT,
        &CPU_USAGE_IRQ,
        &CPU_USAGE_SOFTIRQ,
        &CPU_USAGE_STEAL,
        &CPU_USAGE_GUEST,
        &CPU_USAGE_GUEST_NICE,
    ]
    .iter()
    .map(|v| v.value())
    .sum()
}

fn online_cores(file: &mut std::fs::File) -> Result<usize, ()> {
    let _ = file
        .rewind()
        .map_err(|e| error!("failed to seek to start of file: {e}"))?;

    let mut count = 0;
    let mut raw = String::new();

    let _ = file
        .read_to_string(&mut raw)
        .map_err(|e| error!("failed to read file: {e}"))?;

    for range in raw.split(',') {
        let mut parts = range.split('-');

        let first: Option<usize> = parts
            .next()
            .map(|text| text.parse())
            .transpose()
            .map_err(|e| error!("couldn't parse: {e}"))?;
        let second: Option<usize> = parts
            .next()
            .map(|text| text.parse())
            .transpose()
            .map_err(|e| error!("couldn't parse: {e}"))?;

        if parts.next().is_some() {
            // The line is invalid, report error
            return Err(error!("invalid content in file"));
        }

        match (first, second) {
            (Some(_value), None) => {
                count += 1;
            }
            (Some(start), Some(stop)) => {
                count += stop + 1 - start;
            }
            _ => continue,
        }
    }

    Ok(count)
}

impl Sampler for CpuUsage {
    fn sample(&mut self) {
        let now = Instant::now();
        self.update_online_cores(now);
        self.refresh_counters(now);
        self.refresh_distributions(now);
    }
}
