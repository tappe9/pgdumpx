from __future__ import annotations

from pathlib import Path

path = Path(__file__).with_name("apply_issue17_final.py")
source = path.read_text(encoding="utf-8")

old = '''replace(
    metadata,
    "    copy_statement: Option<&[u8]>,\\n) -> Result<TableDataMetadata, PgDumpError> {\\n",
    "    copy_statement: Option<&[u8]>,\\n    limits: Limits,\\n) -> Result<TableDataMetadata, PgDumpError> {\\n",
)
'''
new = '''metadata_content = read(metadata)
with_limits_marker = "pub(crate) fn parse_table_data_metadata_with_limits(\\n"
with_limits_start = metadata_content.find(with_limits_marker)
if with_limits_start < 0:
    raise RuntimeError("explicit-limits COPY metadata function was not found")
signature_tail = "    copy_statement: Option<&[u8]>,\\n) -> Result<TableDataMetadata, PgDumpError> {\\n"
signature_start = metadata_content.find(signature_tail, with_limits_start)
if signature_start < 0:
    raise RuntimeError("explicit-limits COPY metadata signature tail was not found")
metadata_content = (
    metadata_content[:signature_start]
    + "    copy_statement: Option<&[u8]>,\\n    limits: Limits,\\n) -> Result<TableDataMetadata, PgDumpError> {\\n"
    + metadata_content[signature_start + len(signature_tail):]
)
write(metadata, metadata_content)
'''
if source.count(old) != 1:
    raise RuntimeError("the Issue 17 final patch signature transform was not found")
source = source.replace(old, new, 1)
source = source.replace(
    "#[cfg(test)]\n",
    "#[cfg(test)]\n#[allow(dead_code)]\n",
)
exec(compile(source, str(path), "exec"), {"__name__": "__main__", "__file__": str(path)})
