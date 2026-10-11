extern crate prost_build;

fn main() {
    prost_build::Config::new()
        .enum_attribute(".", "#[derive(strum::EnumIter)]")
        .type_attribute(
            ".",
            "#[derive(serde::Serialize, serde::Deserialize)] #[serde(rename_all = \"snake_case\")]",
        )
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
                    "src/setup_script.proto",
                ],
                ["src/"],
            )
            .expect("Failed to compile protos"),
        )
        .unwrap();
}
