#! /bin/sh
set -exu

cargo install bindgen-cli

echo "#![allow(warnings)]" > src/vk_layer.rs
#echo "#[link(name = \"nvbufsurface\")]" > src/nvbufsurface.rs

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
  cat src/vk_layer.rs

echo "#![allow(warnings)]" > src/vk_beta.rs
#echo "#[link(name = \"nvbufsurface\")]" > src/nvbufsurface.rs

bindgen \
  --default-enum-style=rust \
  --with-derive-default \
  --with-derive-eq \
  --with-derive-hash \
  --with-derive-ord \
  /usr/local/include/vulkan/vulkan.h \
  -- -DVK_ENABLE_BETA_EXTENSIONS=1 \
  >> src/vk_beta.rs

  sed -i 's/extern "C"/extern "system"/g' src/vk_beta.rs
  cat src/vk_beta.rs
