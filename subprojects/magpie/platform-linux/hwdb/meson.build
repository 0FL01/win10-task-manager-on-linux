pkgdatadir = join_paths(get_option('prefix'), get_option('datadir'), 'missioncenter')

gen_hwdb = find_program('generate_hwdb.py', required: true)
hwdb = custom_target('hwdb',
  input: files(
    '20-pci-vendor-model.hwdb',
    '20-usb-vendor-model.hwdb',
  ),
  output: '.',
  install: true,
  install_dir: pkgdatadir,
  command: [gen_hwdb, '--output', '@OUTPUT@', '@INPUT@'],
)
