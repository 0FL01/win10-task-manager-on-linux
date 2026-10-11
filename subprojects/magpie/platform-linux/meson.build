libdrm = dependency('libdrm', method : 'pkg-config', required : true)
libgbm = dependency('gbm', method : 'pkg-config', required : true)
libudev = dependency('libudev', method : 'pkg-config', required : true)

cargo_env += ['MC_PATCH_BINARY=' + find_program('patch', required : true).full_path()]

subdir('hwdb')
