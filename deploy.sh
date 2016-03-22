#!/bin/bash
set -xeuo pipefail

SHA=`cat build.version` make -f Makefile.pbuilder
ls artifacts/*
for DISTRO in $(ls artifacts)
do
  upload_to_packagecloud.sh $DISTRO $(ls artifacts/$DISTRO/*.deb)
done
