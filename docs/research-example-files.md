# VMS Help Source File Examples — Research Notes

## Real-world .HLP files found

### Info-ZIP project (GitHub: LuaDist/zip, LuaDist/unzip)
- `vms/zip_cli.help` — ZIP utility VMS help, multi-level with qualifiers, ~600 lines
- `vms/unzip_cli.help` — UnZip VMS help, 2-level hierarchy with many qualifiers, ~650 lines
- `vms/unzipsfx.hlp` — UnZipSFX self-extracting archive help, simpler 2-level structure
- Good examples of real VMS qualifier-style subtopics (`/CONFIRM`, `/OUTPUT`, etc.)

### Other sources identified but not fetched
- eight-cubed.com — OpenVMS freeware with source code (various utilities)
- VMSSoftware Freeware CDs — archives of VMS community tools
- alan-fay/openvms on GitHub — OpenVMS file system utilities
- vms-ports on SourceForge — centralized VMS open source ports

## Conclusion

Real-world files are available for validation, but our test strategy already defines
purpose-built .hlp fixtures (`minimal.hlp`, `multilevel.hlp`, `qualifiers.hlp`,
`edge-cases.hlp`, etc.) that cover more edge cases than real files typically exercise.

For integration validation, we can fetch Info-ZIP help files as optional test fixtures
later. The core test suite should rely on our controlled fixtures.
