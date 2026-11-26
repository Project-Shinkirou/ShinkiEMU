#!/usr/bin/env python3
"""Replace chardet from its pinned sdist in a disposable Windows Ren'Py SDK."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tarfile
import tempfile
import zipfile


PROBE = """\
import json, platform, chardet, certifi
print(json.dumps({
    'python': platform.python_version(),
    'chardet': chardet.__version__,
    'certifi': certifi.__version__,
    'module': chardet.__file__,
    'replacement': getattr(chardet, '_shinki_replacement_probe', False),
    'encoding': chardet.detect(b'replacement check')['encoding'],
}))
"""


def require(condition, message):
    if not condition:
        raise SystemExit(message)


def verify(path, expected):
    with path.open("rb") as stream:
        actual = hashlib.file_digest(stream, "sha256").hexdigest()
    require(actual == expected, f"SHA-256 mismatch: {path}: {actual}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("sdk_zip", type=Path)
    parser.add_argument("adapter", type=Path, help="exact Cargo-locked Ren'Py checkout")
    parser.add_argument("chardet_sdist", type=Path)
    args = parser.parse_args()
    sdk = json.loads((args.adapter / "shinki/sdk.json").read_text(encoding="utf-8"))
    inventory = json.loads((args.adapter / "licenses/renpy/python-sources.json").read_text(encoding="utf-8"))
    packages = {record["name"]: record for record in inventory["packages"]}
    source = packages["chardet"]
    require(sdk["sha256"] == inventory["sdk_sha256"], "SDK pins differ")
    verify(args.sdk_zip, sdk["sha256"])
    verify(args.chardet_sdist, source["sha256"])

    # Use the SDK launcher, not a host-installed CPython. Never stage games.
    with tempfile.TemporaryDirectory(prefix="renpy-python-replacement-") as out:
        with zipfile.ZipFile(args.sdk_zip) as archive:
            root, = {name.split("/", 1)[0] for name in archive.namelist()}
            native = f"{root}/lib/py3-windows-x86_64/"
            stdlib = f"{root}/lib/python3.12/"
            launcher = native + "python.exe"
            dlls = sdk["windows_x86_64"]
            members = [launcher, *(native + name for name in dlls)]
            members.extend(name for name in archive.namelist() if name.startswith(stdlib))
            archive.extractall(out, members=members)
        stage = Path(out) / root
        for name, digest in dlls.items():
            verify(Path(out) / native / name, digest)

        def run(replaced):
            raw = subprocess.check_output([str(Path(out) / launcher), "-I", "-B", "-c", PROBE],
                                          cwd=stage, text=True, timeout=15)
            result = json.loads(raw)
            filename = "__init__.py" if replaced else "__init__.pyc"
            module = Path(out) / stdlib / "chardet" / filename
            require(Path(result["module"]).resolve() == module.resolve(), raw)
            certifi_version = ".".join(str(int(part)) for part in result["certifi"].split("."))
            require(result["python"] == sdk["python"] and result["chardet"] == source["version"]
                    and certifi_version == packages["certifi"]["version"]
                    and result["replacement"] is replaced and result["encoding"] == "ascii", raw)
            result["module"] = module.relative_to(stage).as_posix()
            print(json.dumps(result), flush=True)

        run(False)
        installed = Path(out) / stdlib / "chardet"
        installed.rename(installed.with_name("chardet-stock"))
        with tarfile.open(args.chardet_sdist) as archive:
            prefix = f"chardet-{source['version']}/chardet/"
            archive.extractall(stage / "rebuild", members=(m for m in archive if m.name.startswith(prefix)),
                               filter="data")
        (stage / "rebuild" / prefix).rename(installed)
        module = installed / "__init__.py"
        with module.open("a", encoding="utf-8") as stream:
            stream.write("\n# Local replacement probe.\n_shinki_replacement_probe = True\n")
        run(True)


if __name__ == "__main__":
    main()
