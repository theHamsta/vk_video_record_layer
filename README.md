Build and run
 - `cargo build --release`
 - `export VK_ADD_LAYER_PATH=$(pwd)/target/release` to add the Vulkan layer JSON to the layer search path
 - Start `vkconfig` in the same terminal and enable `VK_LAYER_THEHAMSTA_video_record`
 - Start an Vulkan application that uses a swapchain, e.g. `vkcube`. Video will be captured from swapchain to files in current folder or path specified in `vkconfig`


TODO:
- [ ] rename vk_layer_video_record
