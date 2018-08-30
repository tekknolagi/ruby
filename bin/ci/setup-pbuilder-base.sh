#!/bin/bash
set -xeo pipefail

TARGET_DIST=${1}
OUTDIR=${2-/var/cache/pbuilder}

if [[ -z "${TARGET_DIST}" ]]; then
  echo "TARGET_DIST must be provided"
  exit 1
fi

TARGET_PATH="${OUTDIR}/${TARGET_DIST}-base.tgz"
if [[ -f "${TARGET_PATH}" ]]; then
  echo "${TARGET_PATH} already exists, skipping build..."
  exit 0;
fi

apt-get update && apt-get install -y pbuilder ubuntu-archive-keyring
mkdir -p ${OUTDIR}
pbuilder --create --distribution ${TARGET_DIST} --basetgz "${TARGET_PATH}" \
  --override-config --components 'main universe multiverse' \
  --mirror http://archive.ubuntu.com/ubuntu/ \
  --extrapackages 'curl apt-transport-https lsb-release ca-certificates wget'
cat <<EOF > /tmp/post-install.sh
#!/bin/bash
curl -s https://packages.shopify.io/install/repositories/shopify/public/script.deb.sh | bash
EOF
chmod +x /tmp/post-install.sh
pbuilder --execute --save-after-exec --basetgz "${TARGET_PATH}" -- /tmp/post-install.sh

echo "Base image build for ${TARGET_DIST} complete..."
ls -lh "${TARGET_PATH}"
