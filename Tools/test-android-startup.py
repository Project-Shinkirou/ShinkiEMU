#!/usr/bin/env python3
"""Exercise the installed Manager's cold-start and background/resume handshake.

Requires an unlocked physical Android device. Stops only ShinkiEMU, never clears
its data. A passing lifecycle gate is not game/UI acceptance: inspect the saved
screenshots and perform game input separately.
"""
import argparse
import os
from pathlib import Path
import subprocess
import time

APP = 'org.shinkirou.shinkiemu'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--serial', required=True)
    parser.add_argument('--adb', type=Path, default=Path(os.environ.get(
        'ANDROID_HOME', '')) / 'platform-tools' / ('adb.exe' if os.name == 'nt' else 'adb'))
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--prepare-tap', type=int, nargs=2, metavar=('X', 'Y'),
                        help='Tap the visible prepare/rescan button at these device coordinates')
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=False)

    def adb(*command, name):
        result = subprocess.run([str(args.adb), '-s', args.serial, *command],
                                capture_output=True, timeout=60, check=True)
        (args.out / name).write_bytes(result.stdout)
        return result.stdout.decode('utf-8', errors='replace')

    policy = adb('shell', 'dumpsys', 'window', 'policy', name='window-policy.txt')
    if 'mIsShowing=true' in policy and 'secure=true' in policy:
        raise RuntimeError('Unlock the device manually before running this test')
    adb('shell', 'am', 'force-stop', APP, name='stop.txt')
    for phase in ('cold-start', 'resume'):
        if phase == 'resume':
            adb('shell', 'input', 'keyevent', 'KEYCODE_HOME', name='background.txt')
        launch = adb('shell', 'am', 'start', '-W', '-n', APP + '/.MainActivity',
                     name=phase + '.txt')
        if 'Status: ok' not in launch or 'Activity: ' + APP + '/.MainActivity' not in launch:
            raise AssertionError('NativeActivity lifecycle did not finish: ' + launch)
        adb('exec-out', 'screencap', '-p', name=phase + '.png')
        adb('shell', 'dumpsys', 'activity', 'activities', name=phase + '-activities.txt')
    if args.prepare_tap:
        before = adb('shell', 'pidof', APP, name='prepare-before-pid.txt').strip()
        adb('shell', 'input', 'tap', *map(str, args.prepare_tap), name='prepare-tap.txt')
        time.sleep(5)
        after = adb('shell', 'pidof', APP, name='prepare-after-pid.txt').strip()
        adb('exec-out', 'screencap', '-p', name='prepare-result.png')
        adb('logcat', '-d', '--pid=' + before, '-v', 'threadtime', name='prepare-logcat.txt')
        if not before or after != before:
            raise AssertionError('Library preparation terminated/restarted the app')
        print('Preparation did not reproduce the renderer-borrow abort; inspect result screenshot.')
    print('Cold-start and background/resume lifecycle handshakes passed.')
    print('Inspect screenshots and test actual game input for UI/game acceptance.')


if __name__ == '__main__':
    main()
