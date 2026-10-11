project('magpie',
        'rust', 'c',
        version : '1.2.0',
        meson_version : '>= 0.63.0',
        default_options : ['warning_level=2', 'werror=false', ],
)

cargo_bin = find_program('cargo')

find_program('cmake', required : true)

c_compiler = meson.get_compiler('c')

cargo_opt  = []
cargo_opt += ['--manifest-path', meson.project_source_root() / 'Cargo.toml']
cargo_opt += ['--target-dir', meson.project_build_root() / 'src']

cargo_env  = []
cargo_env += ['CARGO_HOME=' + meson.project_build_root() / 'cargo-home']

rustflags = []
cargo_features = []

if cargo_features.length() > 0
    cargo_opt += ['--features=' + ','.join(cargo_features)]
endif

if get_option('buildtype') == 'release'
    cargo_opt += ['--release']
    build_opt = 'release'
else
    build_opt = 'debug'
endif

if get_option('b_lto') == true
    if c_compiler.get_id() != 'clang'
        error('LTO is only supported with CLang. Make sure both C and C++ compilers are CLang')
    endif

    if c_compiler.get_linker_id() != 'ld.lld'
        error('LTO is only supported with lld')
    endif

    cargo_env += ['CC=clang']
    cargo_env += ['CXX=clang++']
    cargo_env += ['CFLAGS="-fuse-ld=lld -flto"']
    cargo_env += ['CXXFLAGS="-fuse-ld=lld -flto"']

    rustflags += ['-Clinker=clang', '-Clink-arg=-fuse-ld=lld', '-Clinker-plugin-lto']
endif

cargo_env += ['RUSTFLAGS="' + ' '.join(rustflags) + '"']

if build_machine.system() == 'linux'
    subdir('platform-linux')
endif

subdir('src')
