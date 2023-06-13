#! /bin/sh
set -exu

cargo install bindgen-cli

echo "#![allow(warnings)]" > src/vk_layer.rs

bindgen \
  --default-enum-style=rust \
  --with-derive-default \
  --with-derive-eq \
  --with-derive-hash \
  --with-derive-ord \
  --blocklist-file "/usr/local/include/vk_video/*.h" \
  /usr/local/include/vulkan/vk_layer.h \
  -- \
  >> src/vk_layer.rs

  sed -i 's/extern "C"/extern "system"/g' src/vk_layer.rs

echo "#![allow(warnings)]" > src/vk_beta.rs

bindgen \
  --default-enum-style=rust \
  --with-derive-default \
  --with-derive-eq \
  --with-derive-hash \
  --with-derive-ord \
  --default-macro-constant-type unsigned \
  /usr/local/include/vulkan/vulkan.h \
  --allowlist-var ".*_EXTENSION_NAME" \
  -- -DVK_ENABLE_BETA_EXTENSIONS=1 \
  >> src/vk_beta.rs

  sed -i 's/extern "C"/extern "system"/g' src/vk_beta.rs

cargo fmt
