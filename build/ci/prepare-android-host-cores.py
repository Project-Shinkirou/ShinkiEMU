#!/usr/bin/env python3
"""Build only the standalone Kiri producers needed by this Manager job."""
from pathlib import Path
import subprocess
import tomllib


ROOT = Path(__file__).resolve().parents[2]
TARGETS = ('aarch64-linux-android', 'x86_64-linux-android')
HOST_CORES = ('kirikiroid2-web', 'krkr2')


def xtask(*args):
    subprocess.run(['cargo', 'xtask', *map(str, args)], cwd=ROOT, check=True)


def main():
    inventory = tomllib.loads((ROOT / 'build/cores.toml').read_text(encoding='utf-8'))
    cores = {row['id']: row for row in inventory['cores']}
    for target in TARGETS:
        for plugin_id in HOST_CORES:
            core = cores[plugin_id]
            if target in core.get('static_manager_targets', []):
                continue
            if 'android_dynamic' not in core or target not in core['supported_targets']:
                raise ValueError(f'{plugin_id}/{target}: missing Android host producer policy')
            xtask('deps', '--target', target, '--core', plugin_id)
            xtask('build', '--target', target, '--core', plugin_id, '--release')
            # Validate the actual standalone package layout and ELF closure,
            # reusing the build receipt rather than calling xtask ci again.
            xtask('package', '--target', target, '--core', plugin_id,
                  '--out', f'.work/ci/android-host-cores/{target}/{plugin_id}')


if __name__ == '__main__':
    main()
