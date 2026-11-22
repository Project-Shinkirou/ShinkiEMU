#!/usr/bin/env python3
"""Verify the universal APK and actual KiriKiri host producers.

Static builds retain the archive/DEX checks. Dynamic builds use the exact
standalone receipts and packages produced before Manager. This is an artifact
gate; it does not execute JNI, play audio or enter a game.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import struct
import sys
import tempfile
import tomllib
import zipfile


def command(*args):
    return subprocess.run([str(arg) for arg in args], check=True, text=True,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE).stdout


def dex_methods(text):
    methods = {}
    owner = name = signature = None
    for line in text.splitlines():
        found = re.search(r"Class descriptor\s*:\s*'([^']+)'", line)
        if found:
            owner = found.group(1)
            name = signature = None
            continue
        found = re.search(r"\bname\s*:\s*'([^']+)'", line)
        if found:
            name = found.group(1)
            signature = None
        found = re.search(r"\btype\s*:\s*'([^']+)'", line)
        if found:
            signature = found.group(1)
        if owner and name and signature and signature.startswith('(') and re.search(r"\baccess\s*:", line):
            methods[(owner, name, signature)] = 'NATIVE' in line
    return methods


def symbols(nm, binary, external=True):
    result = set()
    scope = ['--extern-only'] if external else []
    for line in command(nm, '--defined-only', *scope, '--format=posix', binary).splitlines():
        fields = line.split()
        if len(fields) >= 2 and len(fields[1]) == 1:
            result.add(fields[0])
    return result


def digest(path):
    with path.open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def registration_tables(ar, library, contracts):
    """Read actual ELF64 RELA pointers in SDL's compiled JNINativeMethod arrays."""
    members = command(ar, 't', library).splitlines()
    if len(members) != 1:
        raise ValueError('Krkr closure must contain its one resolved native object')
    data = subprocess.run([str(ar), 'p', str(library), members[0]], check=True,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE).stdout
    if data[:6] != b'\x7fELF\x02\x01':
        raise ValueError('Krkr closure is not a little-endian ELF64 object')
    header = struct.unpack_from('<16sHHIQQQIHHHHHH', data)
    first_section = struct.unpack_from('<IIQQQQIIQQ', data, header[6])
    section_count = header[12] or first_section[5]  # ELF extended e_shnum.
    sections = [struct.unpack_from('<IIQQQQIIQQ', data, header[6] + i * header[11])
                for i in range(section_count)]

    def string(section, offset):
        start = sections[section][4] + offset
        return data[start:data.index(b'\0', start)].decode('utf-8')

    tables = {}
    relocations = {}
    for index, section in enumerate(sections):
        if section[1] != 2:  # SHT_SYMTAB
            continue
        symbols = [struct.unpack_from('<IBBHQQ', data, offset)
                   for offset in range(section[4], section[4] + section[5], section[9])]
        extended = next((entry for entry in sections if entry[1] == 18 and entry[6] == index), None)
        if extended:
            symbols = [symbol[:3] + (struct.unpack_from('<I', data, extended[4] + i * 4)[0],) + symbol[4:]
                       if symbol[3] == 0xffff else symbol for i, symbol in enumerate(symbols)]
        for symbol in symbols:
            name = string(section[6], symbol[0])
            if name in {'SDLActivity_tab', 'SDLInputConnection_tab',
                        'SDLAudioManager_tab', 'SDLControllerManager_tab'}:
                tables[name] = symbol
        table_fields = {(symbol[3], offset) for symbol in tables.values()
                        for offset in range(symbol[4], symbol[4] + symbol[5], 8)}
        for relocation in sections:
            if relocation[1] != 4 or relocation[6] != index:  # SHT_RELA
                continue
            for offset in range(relocation[4], relocation[4] + relocation[5], relocation[9]):
                address, info, addend = struct.unpack_from('<QQq', data, offset)
                if (relocation[7], address) not in table_fields:
                    continue
                target = symbols[info >> 32]
                relocations[(relocation[7], address)] = (target, addend, section[6])
    if len(tables) != 4:
        raise ValueError(f'Actual SDL RegisterNatives arrays missing: {sorted(tables)}')
    proof = {}
    for name, symbol in sorted(tables.items()):
        if symbol[5] % 24:
            raise ValueError(f'Invalid JNINativeMethod array size: {name}')
        methods = []
        for offset in range(symbol[4], symbol[4] + symbol[5], 24):
            fields = [relocations[(symbol[3], offset + field)] for field in (0, 8, 16)]
            method = string(fields[0][0][3], fields[0][0][4] + fields[0][1])
            signature = string(fields[1][0][3], fields[1][0][4] + fields[1][1])
            implementation = string(fields[2][2], fields[2][0][0])
            if not fields[2][0][3] or not implementation.startswith('shinki_krkr_'):
                raise ValueError(f'SDL registration does not reference its private implementation: {method}')
            expected = {'class': f'Lorg/krkrsd/app/{name.removesuffix("_tab")};',
                        'name': method, 'signature': signature, 'native': True}
            if expected not in contracts:
                raise ValueError(f'Compiled SDL registration differs from staged DEX contract: {expected}')
            methods.append({'name': method, 'signature': signature, 'implementation': implementation})
        proof[name] = methods
    return proof

HOST_LIBRARIES = {
    'kirikiroid2-web': 'libshinki_emu_kirikiroid2_web.so',
    'krkr2': 'libshinki_emu_krkr2.so',
}


def host_contracts(apk, names, receipt, cores, embedded):
    hashes = receipt['host_compile_input_sha256']
    producers = {}
    contracts = {}
    for plugin_id in HOST_LIBRARIES:
        core = cores[plugin_id]
        prefix = f'android-host/{plugin_id}'
        producer = json.loads((apk.parent / prefix / 'producer.json').read_text(encoding='utf-8'))
        kind = producer.get('artifact_kind')
        if (kind not in ('native-static', 'native-shared')
                or producer.get('plugin_id') != plugin_id
                or producer.get('source_commit') != core['source']['rev']
                or (kind == 'native-static') != (plugin_id in embedded)):
            raise ValueError(f'Android host producer does not match exact APK inventory: {producer}')
        if kind == 'native-shared' and (not core.get('android_dynamic', {}).get('requires_context')
                or any(name.startswith(f'assets/cores/{plugin_id}/') for name in names)):
            raise ValueError(f'Dynamic {plugin_id} lacks its context policy or leaked resources into the APK')
        leaked = f'assets/cores/{plugin_id}/Resources/android-host/'
        if any(name.startswith((leaked, 'assets/' + prefix + '/')) for name in names):
            raise ValueError(f'APK contains compile-only Android host files: {plugin_id}')
        for relative in ('producer.json', 'jni-contract.json'):
            if f'{prefix}/{relative}' not in hashes:
                raise ValueError(f'APK receipt lacks {prefix}/{relative}')
        producers[plugin_id] = producer
        contracts[plugin_id] = json.loads(
            (apk.parent / prefix / 'jni-contract.json').read_text(encoding='utf-8'))
    paths = {name: sha for name, sha in hashes.items() if name.startswith('android-host/')}
    actual = {path.relative_to(apk.parent).as_posix() for path in
              (apk.parent / 'android-host').rglob('*') if path.is_file()}
    if paths.keys() != actual or any(digest(apk.parent / name) != sha for name, sha in paths.items()):
        raise ValueError('Actual Android host compile input files differ from packaging receipt')
    return producers, contracts, paths


def dynamic_table(readelf, library):
    output = command(readelf, '-W', '-d', library)
    needed = set(re.findall(r'\(NEEDED\).*?\[([^\]]+)\]', output))
    runpath = re.findall(r'\(RUNPATH\).*?\[([^\]]+)\]', output)
    soname = re.findall(r'\(SONAME\).*?\[([^\]]+)\]', output)
    return needed, runpath, soname


def dynamic_core(root, target, plugin_id, core, producer, readelf, nm, system_libs):
    receipt_path = root / '.work/build' / target / f'{plugin_id}.json'
    built = json.loads(receipt_path.read_text(encoding='utf-8'))
    manifest = built['core_build']
    library = Path(built['library'])
    if (built['component'] != plugin_id or built['target'] != target or built['profile'] != 'release'
            or built['source_dirty'] or built['features'] or built['source_commit'] != producer['source_commit']
            or manifest['artifact_kind'] != 'native-shared' or manifest['plugin_id'] != plugin_id
            or manifest['target'] != target or manifest['profile'] != built['profile']
            or Path(manifest['native_library']) != library or library.name != HOST_LIBRARIES[plugin_id]):
        raise ValueError(f'Wrong standalone {plugin_id}/{target} build receipt')
    staged = root / '.work/ci/android-host-cores' / target / plugin_id / library.stem
    packaged = json.loads((staged / 'core.json').read_text(encoding='utf-8'))
    identity = packaged['identity']
    pin = core['source']
    files = {entry['path']: entry['sha256'] for entry in packaged['files']}
    if (len(files) != len(packaged['files']) or packaged['schema'] != 'shinki.emu.core_package.v1'
            or packaged['plugin_id'] != plugin_id or packaged['library'] != library.name
            or packaged['abi'] != 'shinki.emu.native.c.v3'
            or identity['adapter'] != {'repo': pin['repo'], 'branch': pin['branch'], 'commit': pin['rev']}
            or identity['upstream'] != pin['upstream_identity']
            or identity['release_sequence'] != pin['release_sequence']):
        raise ValueError(f'Wrong standalone {plugin_id}/{target} package identity')
    actual = {path.relative_to(staged).as_posix() for path in staged.rglob('*') if path.is_file()}
    if (actual != files.keys() | {'core.json'}
            or any(name.startswith(('android-host/', 'Resources/android-host/')) for name in files)
            or any((staged / name).is_symlink() or digest(staged / name) != sha for name, sha in files.items())
            or digest(library) != files[library.name]):
        raise ValueError(f'Standalone {plugin_id}/{target} bytes or layout do not match the exact build')
    provenance = json.loads((staged / 'build.json').read_text(encoding='utf-8'))
    if (provenance.get('schema') != 'shinki.emu.build_identity.v1'
            or any(provenance[key] != built[key] for key in
                   ('component', 'target', 'profile', 'source_commit', 'source_dirty', 'features'))):
        raise ValueError(f'Standalone {plugin_id}/{target} package provenance differs from build receipt')
    asset_count = 0
    for asset in manifest['resources']:
        source = Path(asset['source'])
        for path in (source.rglob('*') if source.is_dir() else [source]):
            if path.is_file():
                name = asset['dest']
                if source.is_dir():
                    name += '/' + path.relative_to(source).as_posix()
                if files.get(name) != digest(path):
                    raise ValueError(f'Standalone {plugin_id}/{target} has wrong resource: {name}')
                asset_count += 1
    if not asset_count or not any(name.startswith(f'licenses/{plugin_id}/android-java/') for name in files):
        raise ValueError(f'Standalone {plugin_id}/{target} lacks resources or host licenses')
    elf = command(readelf, '-W', '-h', '-l', '-n', '--dyn-syms', staged / library.name)
    machine = 'AArch64' if target.startswith('aarch64-') else 'X86-64'
    loads = re.findall(r'^\s*LOAD\s+.*?\s(0x[0-9a-f]+)$', elf, re.MULTILINE)
    if (not re.search(r'Machine:.*' + re.escape(machine), elf, re.IGNORECASE)
            or not re.search(r'Class:\s+ELF64', elf)
            or not re.search(r'Data:.*little endian', elf)
            or not re.search(r'Type:\s+DYN', elf) or not loads
            or any(int(align, 16) < 16384 for align in loads)
            or not re.search(r'Android\s+.*NT_ANDROID_TYPE_IDENT\s*\n\s*description data:\s*1c 00 00 00', elf)
            or not re.search(r'\bFUNC\s+GLOBAL\s+DEFAULT\s+\d+\s+shinki_emu_query$', elf, re.MULTILINE)):
        raise ValueError(f'Standalone {plugin_id}/{target} lacks API 28, ELF64/16 KiB or ABI query evidence')
    compiled = symbols(nm, staged / library.name, external=False)
    if (not any('android_context' in symbol for symbol in compiled)
            or not any('extension' in symbol and symbol.endswith('7context') for symbol in compiled)):
        raise ValueError(f'Standalone {plugin_id}/{target} lacks the compiled Android context table/callback')
    closure = {}
    pending = [staged / name for name in files
               if Path(name).name.endswith('.so') or '.so.' in Path(name).name]
    while pending:
        path = pending.pop()
        name = path.relative_to(staged).as_posix()
        if name in closure:
            continue
        needed, runpath, soname = dynamic_table(readelf, path)
        private = needed - system_libs
        if ((soname and soname != [path.name]) or (path.name in system_libs)
                or (private and runpath != ['$ORIGIN'])):
            raise ValueError(f'Standalone {plugin_id}/{target} has a bad SONAME/device stub/runpath: {name}')
        for dep in private:
            sibling = path.with_name(dep)
            if sibling.relative_to(staged).as_posix() not in files:
                raise ValueError(f'Standalone {plugin_id}/{target} lacks {name} dependency {dep}')
            pending.append(sibling)
        closure[name] = sorted(needed)
    return {'library_sha256': files[library.name], 'source_commit': built['source_commit'],
            'package_manifest_sha256': digest(staged / 'core.json'), 'resources': asset_count,
            'android_api': 28, 'context_table': 'compiled; not invoked', 'needed': closure}


def verify_static_kiri(nm, ar, objdump, native, contracts, linked, target):
    if native['artifact_kind'] != 'native-static' or native['target'] != target:
        raise ValueError('Krkr lacks an actual target native-static receipt')
    library = Path(native['native_library'])
    defined = symbols(nm, library)
    registrations = registration_tables(ar, library, contracts)
    public = {'shinki_emu_kirikiroid2_web_query', 'shinki_emu_kirikiroid2_web_set_resource_root',
              'shinki_emu_kirikiroid2_web_android_initialize'}
    if not public.issubset(defined) or any(name not in public and not name.startswith('shinki_krkr_') for name in defined):
        raise ValueError(f'Krkr native closure export/namespace failure: {target}')
    for marker in ('shinki_krkr_SDL_Init', 'shinki_krkr_shinki_krkr_sdl_on_load'):
        if marker not in defined:
            raise ValueError(f'Krkr original SDL implementation missing: {marker}')
    cocos_jni = []
    for contract in contracts:
        if contract['native'] and contract['class'].startswith('Lorg/krkrc2dx/lib/'):
            original_class = contract['class'][1:-1].replace('org/krkrc2dx/', 'org/cocos2dx/')
            implementation = 'shinki_krkr_Java_' + original_class.replace('/', '_') + '_' + contract['name']
            if implementation not in defined:
                raise ValueError(f'Krkr original Cocos JNI implementation missing: {implementation}')
            cocos_jni.append({**contract, 'implementation': implementation})
    if not any(name.startswith('shinki_krkr_avcodec_') for name in defined):
        raise ValueError('Krkr private FFmpeg implementation missing')
    for implementation in ('_ZN7cocos2d8Director', '_ZN4oboe', 'alSourcePlay'):
        if not any(implementation in name for name in defined):
            raise ValueError(f'Krkr real renderer/audio implementation missing: {implementation}')
    if any(name.endswith('_JNI_OnLoad') for name in defined):
        raise ValueError('Krkr retained a second JNI_OnLoad instead of its explicit private loader')
    all_defined = symbols(nm, library, external=False)
    spine = sorted(name for name in all_defined if re.search(r'spAnimation|spSkeleton|spine|SkeletonRenderer', name, re.IGNORECASE))
    if spine:
        raise ValueError(f'Actual Krkr static closure includes Spine runtime; applicable grant or unused-module exclusion required: {spine[:8]}')
    exit_symbols = [name for name in defined if 'TVPExitApplication' in name]
    env_symbols = [name for name in defined if name.endswith('9JniHelper6getEnvEv')]
    if len(exit_symbols) != 1 or len(env_symbols) != 1:
        raise ValueError('Krkr exit/environment implementations are missing or ambiguous')
    exit_code = command(objdump, '--reloc', '--disassemble-symbols=' + exit_symbols[0], library)
    env_code = command(objdump, '--reloc', '--disassemble-symbols=' + env_symbols[0], library)
    if re.search(r'\b(exit|_exit|quick_exit|abort)\b', exit_code):
        raise ValueError('Embedded Krkr exit implementation still references process termination')
    if 'pthread_getspecific' in env_code:
        raise ValueError('Cocos getEnv still reuses a cached environment across host attachment scopes')
    if not public.issubset(linked):
        raise ValueError(f'Manager did not link the actual Krkr query/host: {target}')
    return {'archive': str(library), 'archive_sha256': digest(library),
            'private_symbols': len(defined) - len(public),
            'compiled_register_natives': registrations, 'spine_runtime_symbols': spine,
            'compiled_cocos_jni_implementations': cocos_jni,
            'embedded_exit_no_process_fallback': True, 'cocos_getenv_no_cached_environment': True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    selection = parser.add_mutually_exclusive_group(required=True)
    selection.add_argument('--apk', type=Path)
    selection.add_argument('--ci-report', type=Path)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    apk = args.apk
    if args.ci_report:
        report = json.loads(args.ci_report.read_text(encoding='utf-8'))
        if report['status'] != 'built' or not report['artifact']:
            raise ValueError('Manager CI has not packaged its actual APK')
        apk = Path(report['artifact'])
    sdk = Path(os.environ['ANDROID_HOME'])
    ndk = Path(os.environ.get('ANDROID_NDK_HOME', sdk / 'ndk/30.0.16248370'))
    host = 'windows-x86_64' if sys.platform == 'win32' else ('darwin-x86_64' if sys.platform == 'darwin' else 'linux-x86_64')
    suffix = '.exe' if sys.platform == 'win32' else ''
    nm = ndk / 'toolchains/llvm/prebuilt' / host / 'bin' / ('llvm-nm' + suffix)
    dexdump = sdk / 'build-tools/37.0.0' / ('dexdump' + suffix)
    aapt = sdk / 'build-tools/37.0.0' / ('aapt2' + suffix)
    strip = nm.with_name('llvm-strip' + suffix)
    ar = nm.with_name('llvm-ar' + suffix)
    objdump = nm.with_name('llvm-objdump' + suffix)
    readelf = nm.with_name('llvm-readelf' + suffix)
    ndk_libs = json.loads((ndk / 'meta/system_libs.json').read_text(encoding='utf-8'))
    system_libs = {name for name, api in ndk_libs.items() if int(api) <= 28}
    prefix = 'assets/cores/kirikiroid2-web/Resources/'
    proof = {'apk': str(apk.resolve()), 'sha256': digest(apk), 'device_launch': 'not run (artifact gate only)', 'abis': {}}
    receipt = json.loads(apk.with_name('android.json').read_text(encoding='utf-8'))
    if (receipt.get('schema') != 'shinki.emu.android_package.v1'
            or receipt.get('apk') != apk.name or receipt.get('sha256') != proof['sha256']):
        raise ValueError('APK differs from its actual universal packaging receipt')
    head = command('git', '-C', args.root, 'rev-parse', 'HEAD').strip()
    proof['source_commit'] = head
    inventory = tomllib.loads((args.root / 'build/cores.toml').read_text(encoding='utf-8'))
    cores = {core['id']: core for core in inventory['cores']}
    selected = {plugin_id: core for plugin_id, core in cores.items()
                if 'aarch64-linux-android' in core.get('static_manager_targets', [])}
    signer = sdk / 'build-tools/37.0.0' / ('apksigner.bat' if sys.platform == 'win32' else 'apksigner')
    proof['signature_verification'] = command(signer, 'verify', '--verbose', '--print-certs', apk)
    with zipfile.ZipFile(apk) as archive, tempfile.TemporaryDirectory(prefix='krkr-apk-') as temporary:
        names = set(archive.namelist())
        embedded = json.loads(archive.read('assets/embedded-cores.json'))
        ids = {core['id'] for core in embedded}
        if ids != set(selected) or len(embedded) != len(selected):
            raise ValueError('APK static consumers differ from exact product inventory')
        for entry in embedded:
            if 'source' in selected[entry['id']] and entry['source']['rev'] != selected[entry['id']]['source']['rev']:
                raise ValueError(f'APK fork pin differs from product inventory: {entry["id"]}')
        cmvs_licenses = {}
        if 'shinki.emu.cmvs' in ids:
            for relative in selected['shinki.emu.cmvs']['licenses']:
                content = archive.read('assets/licenses/shinki.emu.cmvs/' + relative)
                if content != (args.root / relative).read_bytes():
                    raise ValueError(f'APK CMVS license differs from source: {relative}')
                cmvs_licenses[relative] = hashlib.sha256(content).hexdigest()
        proof['cmvs_licenses_sha256'] = cmvs_licenses
        bundled = archive.read('assets/bundled.manifest')
        if len(bundled) > 1024 * 1024:
            raise ValueError('APK bundled manifest exceeds the host bound')
        resource_hashes_all = {}
        largest = 0
        for relative in bundled.decode('utf-8').splitlines():
            if not relative:
                continue
            name = 'assets/' + relative
            size = archive.getinfo(name).file_size
            if size > 32 * 1024 * 1024:
                raise ValueError(f'APK resource exceeds the streamed host bound: {relative}')
            largest = max(largest, size)
            with archive.open(name) as stream:
                resource_hashes_all[relative] = hashlib.file_digest(stream, 'sha256').hexdigest()
        if 'renpy' in ids:
            for relative in ('renpy/__init__.py', 'shinki_renpy_bridge.py', 'lib/python3.12/encodings/__init__.py'):
                if 'cores/renpy/Resources/' + relative not in resource_hashes_all:
                    raise ValueError(f'APK lacks original Renpy runtime resource: {relative}')
            if any(name.startswith('assets/cores/renpy/Resources/') and
                   name.endswith(('.so', '.pyd', '.dll', '.dylib')) for name in names):
                raise ValueError('APK static Renpy resources contain a dynamic module')
        elif any(name.startswith('assets/cores/renpy/') for name in names):
            raise ValueError('Standalone Renpy runtime leaked into the APK')
        for font in ('NotoSansCJKjp-Regular.otf', 'NotoSansCJKjp-Bold.otf'):
            relative = 'resources/common/font/' + font
            original = args.root / 'ThirdParty/impacto' / relative
            packaged = 'cores/impacto/Resources/' + relative
            if resource_hashes_all.get(packaged) != digest(original):
                raise ValueError(f'APK Impacto font differs from original: {font}')
        proof['bundled_resources'] = {'files': len(resource_hashes_all), 'largest_bytes': largest,
                                      'sha256': resource_hashes_all}
        proof['embedded_cores'] = sorted(ids)
        producers, host_methods, compile_hashes = host_contracts(apk, names, receipt, cores, ids)
        proof['host_producers'] = producers
        proof['host_compile_input_sha256'] = compile_hashes
        methods = {}
        for name in sorted(names):
            if re.fullmatch(r'classes\d*\.dex', name):
                dex = Path(temporary) / name
                dex.write_bytes(archive.read(name))
                methods.update(dex_methods(command(dexdump, dex)))
        for plugin_id, contracts in host_methods.items():
            for contract in contracts:
                key = (contract['class'], contract['name'], contract['signature'])
                if key not in methods or methods[key] != contract['native']:
                    raise ValueError(f'APK {plugin_id} DEX/JNI signature mismatch: {contract}')
        proof['dex_jni_contracts'] = sum(map(len, host_methods.values()))
        if 'kirikiroid2-web' in ids:
            for suffix_required in ('.csb', '.ttf'):
                if not any(name.startswith(prefix) and name.endswith(suffix_required) for name in names):
                    raise ValueError(f'APK lacks original Cocos resource {suffix_required}')
            original_resources = args.root / 'ThirdParty/kirikiri2/ui/cocos-studio'
            resource_hashes = {}
            for resource in sorted(original_resources.rglob('*')):
                if resource.is_file():
                    relative = resource.relative_to(original_resources).as_posix()
                    content = archive.read(prefix + relative)
                    if hashlib.sha256(content).hexdigest() != digest(resource):
                        raise ValueError(f'APK changed an original Cocos resource: {relative}')
                    resource_hashes[relative] = digest(resource)
            if not resource_hashes:
                raise ValueError('Exact Krkr source resources are unavailable')
            proof['original_resources_sha256'] = resource_hashes
            for notice in ('LICENSE', 'FMOD.txt', 'libbpg-MIT.txt', 'UnRAR.txt', 'minizip-zlib.txt',
                           'LICENSE_cocos2d-x.txt', 'LICENSE_cocosdenshion.txt',
                           'LICENSE_com.android.vending.expansion.zipfile.txt', 'SDL-LICENSE.txt'):
                if not any(name.startswith('assets/licenses/') and name.endswith('/' + notice) and archive.getinfo(name).file_size for name in names):
                    raise ValueError(f'APK lacks authentic native/Java notice: {notice}')
        manifest = command(aapt, 'dump', 'xmltree', apk, '--file', 'AndroidManifest.xml')
        activities = []
        activity_indent = None
        for line in manifest.splitlines():
            indent = len(line) - len(line.lstrip())
            if re.search(r'E: activity-alias(?:\s|$)', line):
                raise ValueError('APK adds an Activity alias')
            if re.search(r'E:\s', line):
                activity_indent = indent if re.search(r'E: activity(?:\s|$)', line) else None
                if activity_indent is not None:
                    activities.append(None)
            elif activity_indent is not None and indent <= activity_indent and line.strip():
                activity_indent = None
            if activity_indent is not None and 'android:name' in line:
                found = re.search(r'=\s*"([^"]+)"', line)
                if found:
                    activities[-1] = found.group(1)
        if activities != ['org.shinkirou.shinkiemu.MainActivity']:
            raise ValueError(f'APK entry Activities changed: {activities}')
        proof['entry_activities'] = activities
        stage_coverage = set()
        for target, abi in [('aarch64-linux-android', 'arm64-v8a'), ('x86_64-linux-android', 'x86_64')]:
            if f'lib/{abi}/libshinki_emu_manager.so' not in names:
                raise ValueError(f'APK lacks actual Manager ABI: {abi}')
            if any(f'lib/{abi}/{name}' in names for name in HOST_LIBRARIES.values()):
                raise ValueError('APK contains a standalone Kiri library')
            if any(f'lib/{abi}/{name}' in names for name in ('libGLESv1_CM.so', 'libcamera2ndk.so')):
                raise ValueError('APK bundles Android system import stubs')
            built = json.loads((args.root / '.work/build' / target / 'manager.json').read_text(encoding='utf-8'))
            if built['source_dirty'] or built['source_commit'] != head:
                raise ValueError('Manager build receipt is dirty or does not match exact product HEAD')
            commits = {entry['source_commit'] for entry in proof['abis'].values()}
            if commits and built['source_commit'] not in commits:
                raise ValueError('APK ABI build receipts name different product commits')
            packaged_library = Path(temporary) / f'manager-{abi}.so'
            packaged_library.write_bytes(archive.read(f'lib/{abi}/libshinki_emu_manager.so'))
            stripped_library = Path(temporary) / f'manager-{abi}-stripped.so'
            command(strip, '--strip-unneeded', '-o', stripped_library, built['library'])
            packaged_sha256 = digest(packaged_library)
            if digest(stripped_library) != packaged_sha256:
                raise ValueError(f'APK Manager ELF differs from its actual build receipt: {abi}')
            embedded_builds = {core['plugin_id']: core for core in built['embedded_core_builds']}
            if set(embedded_builds) != ids:
                raise ValueError(f'APK static inventory differs from the compiled {abi} receipts')
            linked = symbols(nm, Path(built['library']), external=False)
            manager_needed, _, _ = dynamic_table(readelf, packaged_library)
            if manager_needed.intersection(HOST_LIBRARIES.values()):
                raise ValueError(f'Manager {abi} has a link-time dependency on a standalone Kiri core')
            host_proof = {}
            for plugin_id, producer in producers.items():
                namespace = HOST_LIBRARIES[plugin_id].removeprefix('lib').removesuffix('.so')
                private_prefix = 'shinki_krkr_' if plugin_id == 'kirikiroid2-web' else 'shinki_krkr2_'
                if producer['artifact_kind'] == 'native-shared':
                    if any(symbol.startswith((namespace + '_', private_prefix)) for symbol in linked):
                        raise ValueError(f'Manager {abi} unexpectedly links the {plugin_id} native implementation')
                    host_proof[plugin_id] = dynamic_core(args.root, target, plugin_id, cores[plugin_id],
                                                        producer, readelf, nm, system_libs)
                elif plugin_id == 'kirikiroid2-web':
                    host_proof[plugin_id] = verify_static_kiri(
                        nm, ar, objdump, embedded_builds[plugin_id], host_methods[plugin_id], linked, target)
                else:
                    native = embedded_builds[plugin_id]
                    if (native['artifact_kind'] != 'native-static' or native['target'] != target
                            or namespace + '_query' not in linked):
                        raise ValueError(f'Manager {abi} lacks the selected embedded {plugin_id} implementation')
                    host_proof[plugin_id] = {'archive_sha256': digest(Path(native['native_library']))}
            cmvs_queries = sorted(name for name in linked if 'shinki_emu_cmvs' in name and 'cmvs_query' in name)
            if 'shinki.emu.cmvs' in ids and not cmvs_queries:
                raise ValueError(f'Manager did not link the actual CMVS static query: {target}')
            native_markers = ['Java_org_shinkirou_shinkiemu_MainActivity_nativeOnPickResult',
                              'shinki_emu_impacto_query', 'shinki_impacto_lua_newstate']
            if 'renpy' in ids:
                native_markers.extend(('shinki_emu_renpy_query', 'Py_InitializeFromConfig',
                                       'shinki_renpy_SDL_Init', 'shinki_renpy_avcodec_open2'))
                if not any(name.startswith('shinki_renpy__Z') for name in linked):
                    raise ValueError('Manager lacks original isolated Renpy C++ implementation')
            for marker in native_markers:
                if marker not in linked:
                    raise ValueError(f'Manager native consumer missing: {target}: {marker}')
            impacto = next(core for core in built['embedded_core_builds'] if core['plugin_id'] == 'impacto')
            lua = Path(impacto['native_library']).with_name('libminilua.a')
            lua_defined = symbols(nm, lua)
            if 'shinki_impacto_lua_ident' not in lua_defined or 'lua_ident' in lua_defined:
                raise ValueError('Impacto compiled input lacks isolated original Lua data')
            staged = Path(built['target_dir']) / 'android-inputs/assets'
            actual_sidecars = {path.relative_to(staged.parent).as_posix() for path in
                               (staged.parent / 'android-host').rglob('*') if path.is_file()}
            if actual_sidecars != compile_hashes.keys():
                raise ValueError(f'APK host input set differs from {abi} build stage')
            for relative, expected_hash in compile_hashes.items():
                if digest(staged.parent / relative) != expected_hash:
                    raise ValueError(f'APK host input differs from {abi} build stage: {relative}')
            for relative, packaged_hash in resource_hashes_all.items():
                original = staged / relative
                if original.is_file():
                    stage_coverage.add(relative)
                    if digest(original) != packaged_hash:
                        raise ValueError(f'APK resource differs from actual ABI stage: {abi}: {relative}')
            proof['abis'][abi] = {'source_commit': built['source_commit'],
                                 'packaged_manager_sha256': packaged_sha256,
                                 'cmvs_static_query_symbols': cmvs_queries, 'hosts': host_proof}
        missing_stage = set(resource_hashes_all) - stage_coverage
        if missing_stage:
            raise ValueError(f'APK resources lack actual ABI staged inputs: {sorted(missing_stage)}')
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(proof, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(proof, indent=2))


if __name__ == '__main__':
    main()
