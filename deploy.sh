#!/bin/bash
set -xeuo pipefail

SHA=`cat build.version` make -f Makefile.pbuilder
ls artifacts/*
for DISTRO in $(ls artifacts)
do
  push_debs_to_stable.sh $DISTRO $(ls artifacts/$DISTRO/*.deb)
done
