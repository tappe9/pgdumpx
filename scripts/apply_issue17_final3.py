from __future__ import annotations

from pathlib import Path

path = Path(__file__).with_name("apply_issue17_final2.py")
source = path.read_text(encoding="utf-8")
source = source.replace(
    "#[cfg(test)]\\n",
    "#[cfg(test)]\\n#[allow(dead_code)]\\n",
)
exec(compile(source, str(path), "exec"), {"__name__": "__main__", "__file__": str(path)})
