#!/bin/bash

# Usage: ./package.sh $tar_ball_name $bin_path
# This script will create a tar ball for the specified arch.

set -e
set -x

DESTINATION="packaging"
WORK_DIR="_packaging_internal"

tar_ball_name=$1
bin_path=$2

if [ -z "$tar_ball_name" ]
then
  echo "ERROR: ./package.sh requires the first positional param to be the name of the tar ball."
  exit 1
fi

if [ -z "$bin_path" ]
then
  echo "ERROR: ./package.sh requires the second positional param to be the path to a binary."
  exit 1
fi

rm -rf $WORK_DIR
mkdir -p $WORK_DIR
cp $bin_path $WORK_DIR/

mkdir -p $(dirname "$DESTINATION/$tar_ball_name")

pushd $WORK_DIR
  tar -czvf ../$DESTINATION/$tar_ball_name *
popd

rm -rf $WORK_DIR
