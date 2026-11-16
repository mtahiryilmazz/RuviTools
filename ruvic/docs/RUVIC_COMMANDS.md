# RUVIC Komut Başvurusu

- `ruvic probe <INPUT> [--manifest <PATH>] [--json] [--streams] [--chapters]`
- `ruvic convert <INPUT> -o <OUTPUT> [--preset <NAME>] [--video <CODEC>] [--audio <CODEC>] [--quality <VAL>] [--speed <PRESET>] [--hwaccel <MODE>] [--overwrite]`
- `ruvic remux <INPUT> -o <OUTPUT> [--copy] [--overwrite]`
- `ruvic audio <INPUT> -o <OUTPUT> [--to <FMT>] [--copy] [--overwrite]`
- `ruvic trim <INPUT> --start <TIME> (--end <TIME> | --duration <SECS>) -o <OUTPUT> [--reencode] [--overwrite]`
- `ruvic thumbnail <INPUT> --at <TIME> -o <OUTPUT> [--overwrite]`
- `ruvic batch <INPUT_DIR> -o <OUTPUT_DIR> [--preset <NAME>] [--ext <EXT>] [--skip-errors] [--overwrite]`
- `ruvic preset list | show <NAME> | validate <PATH>`
- `ruvic config path | show | validate | init [--overwrite]`
- `ruvic doctor [--json]`
