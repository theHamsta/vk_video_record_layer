use std::process::Command;

use anyhow::bail;
use glob::glob;
use std::env;
use std::path::Path;
use std::path::PathBuf;

fn get_output_path() -> PathBuf {
    //<root or manifest path>/target/<profile>/
    let manifest_dir_string = env::var("CARGO_MANIFEST_DIR").unwrap();
    let build_type = env::var("PROFILE").unwrap();
    let path = Path::new(&manifest_dir_string)
        .join("target")
        .join(build_type);
    path
}

pub fn main() -> anyhow::Result<()> {
    println!("cargo:rerun-if-changed=shaders");
    println!("cargo:rerun-if-changed=vk_video_record.json");

    std::fs::copy(
        "./vk_video_record.json",
        get_output_path().join("vk_video_record.json"),
    )
    .expect("Failed to copy layer JSON manifest to target dir");

    for entry in glob("./shaders/*").expect("Failed to read glob pattern") {
        let entry = entry?;
        if let Some(ext) = entry.extension() {
            let output = match ext.to_str() {
                Some("cu") => Command::new("nvcc")
                    .args([
                        "-O3",
                        "--ptx",
                        "-lineinfo",
                        "-o",
                        &format!("{}.ptx", entry.to_string_lossy()),
                        entry.to_str().unwrap(),
                    ])
                    .output()?,
                Some("hlsl") => Command::new("dxc")
                    .args([
                        "-T",
                        "cs_6_5",
                        "-O3",
                        "-spirv",
                        "-fspv-target-env=vulkan1.3",
                        "-Zi",
                        "-Fo",
                        &format!("{}.spirv", entry.to_string_lossy()),
                        entry.to_str().unwrap(),
                    ])
                    .output()?,
                Some("spirv") | Some("ptx") | Some("cubin") => continue,
                _ => Command::new("glslc")
                    .args([
                        entry.to_str().unwrap(),
                        "--target-env=vulkan1.3",
                        "-g",
                        "-o",
                        &format!("{}.spirv", entry.to_string_lossy()),
                        "-O",
                    ])
                    .output()?,
            };

            eprintln!("{}", String::from_utf8(output.stdout)?);
            eprintln!("{}", String::from_utf8(output.stderr)?);
            if !output.status.success() {
                bail!("Failed to run shader compiler: {}", output.status);
            }
        }
    }

    #[cfg(feature = "nvpro_sample_gop")]
    {
        println!("cargo:rerun-if-changed=src/VkVideoGopStructure.cpp");
        println!("cargo:rerun-if-changed=src/VkVideoGopStructure.h");
        cc::Build::new()
            .file("src/VkVideoGopStructure.cpp")
            .cpp(true)
            .flag_if_supported("-fkeep-inline-functions")
            .include("src")
            .compile("vkvideogop");
    }

    Ok(())
}
