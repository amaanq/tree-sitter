#!/bin/bash -eu
if [[ $BUILD_CMD == cross ]]; then
  cross.sh npm "$@"
else
  exec npm "$@"
fi
