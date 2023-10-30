#!/bin/bash

# Usage: ./dev-quick-deploy.sh
# This is a script to speed-up development by doing a hot reload.

set -e
set -x

export AWS_PROFILE=dev
export AWS_REGION=us-west-2
export DEPLOY_ORG=preprod
export MOMENTO_ENV_NAME=dev
export ACCOUNT_ID=$(aws sts get-caller-identity --output text --query 'Account')
export CODE_REVISION_ID=latest
export DESTINATION="packaging"
# Change this to amd64 if you're not running graviton in your dev cell
export DEV_CELL_TARBALL="arm64v8/rezolus.tar.gz"
# Change this to  if you're not running graviton in your dev cell
export BUILD_ARCH="aarch64-unknown-linux-gnu"

if [[ "$BUILD_ARCH" == "aarch64-unknown-linux-gnu" ]]; then
  CODEBUILD_IMAGE_NAME="401011790710.dkr.ecr.us-west-2.amazonaws.com/momento-codebuild-2021-11-arm"
  CODEBUILD_IMAGE_TAG="latest"
  docker pull ${CODEBUILD_IMAGE_NAME}:${CODEBUILD_IMAGE_TAG}
  docker run -it -v `pwd`:/rezolus -w /rezolus ${CODEBUILD_IMAGE_NAME} bash -c 'export PATH=$PATH:$HOME/.cargo/bin && make pipeline-build-arm64v8'
else
  echo "This script doesn't support native builds for $BUILD_ARCH, please add support for it"
  exit 1
fi  

S3_DESTINATION="s3://${CLOUDPROVIDER}artifact-$MOMENTO_ENV_NAME-$ACCOUNT_ID-$AWS_REGION/rezolus/snapshot/latest"
aws s3 sync "$DESTINATION" $S3_DESTINATION

cat << EOF > hot-reload.json
{
    "Parameters": {
      "commands": [
        "#!/bin/bash",
        "set -x",
        "set -e",
        "pushd \`mktemp -d\`",
        "aws s3 cp --no-progress ${S3_DESTINATION}/${DEV_CELL_TARBALL} rezolus.tar.gz",
        "tar --extract --file rezolus.tar.gz --directory /usr/local/bin/",
        "systemctl restart rezolus.service",
        "systemctl status rezolus.service",
        "popd"
      ]
    }
}
EOF

sh_command_id=$(aws ssm send-command \
    --document-name "AWS-RunShellScript" \
    --targets "Key=tag:aws:cloudformation:stack-name,Values=routing-stack-dev" \
    --cli-input-json file://hot-reload.json \
    --output text --query "Command.CommandId")

sleep 3

aws ssm list-command-invocations \
    --command-id "$sh_command_id" \
    --details
