cargo_build = custom_target(
    'cargo-build',
    build_by_default : true,
    build_always_stale : true,
    output : 'missioncenter-magpie',
    console : true,
    install : true,
    install_dir : get_option('bindir'),
    command : [
        'sh', '-c', 'env '
        + ' env '.join(cargo_env)
        + ' ' + cargo_bin.full_path() + ' build ' + ' '.join(cargo_opt)
        + ' ' + '&&'
        + ' ' + 'cp ' + meson.project_build_root() + '/src' + '/' + build_opt + '/magpie' + ' ' + '@OUTPUT@'
    ]
)
