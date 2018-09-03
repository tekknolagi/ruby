#!/bin/bash
NAME=$(cat NAME_TO_BUILD 2>/dev/null)
VERSION=${VERSION:-$(cat VERSION 2>/dev/null || echo 1)}

if [[ -z "${NAME}" || "${NAME}" == "ruby-shopify-X.Y.Z" ]]; then
  >&2 echo "Build name must be set, check the README"
  exit 1
fi

git clone git@github.com:shopify/ruby.git #TODO(DazWorrall): In lieu of history: full
cd ruby && ../ruby_build --name "${NAME}" --version "${VERSION}"
