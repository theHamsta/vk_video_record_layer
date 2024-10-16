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

echo "#![allow(warnings)]" > src/gop_gen.rs

bindgen \
  --default-enum-style=rust \
  --with-derive-default \
  --with-derive-eq \
  --with-derive-hash \
  --with-derive-ord \
  --generate-inline-functions \
  --allowlist-type VkVideoGopStructure \
  --allowlist-function VkVideoGopStructure_new \
  --allowlist-function VkVideoGopStructure_destroy \
  src/VkVideoGopStructure.h \
  -- -x c++ -fkeep-inline-functions \
  >> src/gop_gen.rs

cargo fmt
