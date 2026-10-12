extern crate prost_build;

fn main() {
    prost_build::Config::new()
        .enum_attribute(".", "#[derive(strum::EnumIter)]")
        .type_attribute(
            ".",
            "#[derive(serde::Serialize, serde::Deserialize)] #[serde(rename_all = \"snake_case\")]",
        )
        // Fields added after recorded fixtures existed
        .field_attribute(".magpie.memory.Memory.swap_devices", "#[serde(default)]")
        .field_attribute(".magpie.memory.Memory.zswap_enabled", "#[serde(default)]")
        .compile_fds(
            protox::compile(
                [
                    "src/about.proto",
                    "src/apps.proto",
                    "src/battery.proto",
                    "src/common.proto",
                    "src/cpu.proto",
                    "src/gpus.proto",
                    "src/ipc.proto",
                    "src/memory.proto",
                    "src/network.proto",
                    "src/processes.proto",
                    "src/services.proto",
                ],
                ["src/"],
            )
            .expect("Failed to compile protos"),
        )
        .unwrap();
}
