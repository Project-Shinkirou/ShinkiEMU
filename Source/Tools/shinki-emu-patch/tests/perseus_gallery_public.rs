#[path = "support/musica.rs"]
mod native;

use sha2::{Digest, Sha256};
use shinki_emu_patch::{PatchErrorKind, PatchMode, PatchRequest, apply};
use std::{collections::BTreeSet, fs, path::Path, sync::{Arc, atomic::AtomicBool}};

const ROOT: u32 = 0x400000;
const CG: usize = 0x200;
const CG_ROWS: usize = 0x210;
const FLASH: usize = 0x290;
const MUSIC: usize = 0x2a0;
const MOVIE: usize = 0x370;
const DATA: usize = 0x390;
const SECTION: usize = 0x178;

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn va(offset: usize) -> u32 {
    ROOT + 0x1000 + (offset - 0x200) as u32
}


fn op(image: &mut [u8], at: usize, tag: &[u8], value: u32) {
    image[at..at + tag.len()].copy_from_slice(tag);
    put32(image, at + tag.len(), value);
}

fn flash_call(image: &mut [u8], start: usize, col: u8, row: u8) -> usize {
    image[start..start + 3].copy_from_slice(&[0x6a, row, 0x53]);
    let call = if col == 0 {
        image[start + 3..start + 5].copy_from_slice(&[0x33, 0xc9]);
        start + 5
    } else {
        op(image, start + 3, &[0xb9], col.into());
        start + 8
    };
    let next = call + 5;
    op(image, call, &[0xe8], (0xe00i32 - next as i32) as u32);
    next
}

fn geometry(image: &mut [u8]) {
    let root = [[20, 40, 100, 80], [20, 90, 100, 130], [20, 140, 100, 180],
        [20, 190, 100, 230], [20, 240, 100, 280]];
    for (i, rect) in root.into_iter().enumerate() {
        for (j, edge) in rect.into_iter().enumerate() {
            let at = 0x800 + i * 32 + j * 8;
            op(image, at, &[0xb8], edge);
            if j < 3 {
                image[at + 5..at + 8].copy_from_slice(&[0x89, 0x45, 0xd0 + j as u8 * 4]);
            }
        }
    }
    for (i, value) in [64, 240, 416, 592, 96, 208, 320, 432, 544].into_iter().enumerate() {
        let disp = if i < 4 { 104 + i * 4 } else { 84 + (i - 4) * 4 };
        op(image, 0x8a0 + i * 10, &[0xc7, 0x85, disp as u8, 0xff, 0xff, 0xff], value);
    }
    op(image, 0x900, &[0xbe], 4);
    image[0x910..0x913].copy_from_slice(&[0x83, 0xf8, 20]);
    op(image, 0x920, &[0x8d, 0x90], 136);
    image[0x930..0x933].copy_from_slice(&[0x8d, 0x51, 80]);
    op(image, 0x940, &[0x68], 555);
    op(image, 0x950, &[0xc7, 0x45, 0xa4], 371);
    op(image, 0x957, &[0xc7, 0x45, 0xa8], 640);
    for (i, value) in [384,640,496,704,520,640,632,704,656,640,728,704].into_iter().enumerate() {
        put32(image, 0x960 + i * 4, value);
    }
    let mut at = 0xa00;
    for col in 0..3 { at = flash_call(image, at, col, 1); }
    let remaining = 0xa80 - (at + 4);
    image[at..at + 4].copy_from_slice(&[0x6a, 3, 0xeb, remaining as u8]);
    let mut at = 0xa43;
    for (col, row) in [(0,1),(1,1),(2,1),(3,1),(0,2)] {
        at = flash_call(image, at, col, row);
    }
    assert_eq!(at, 0xa7e);
    assert_eq!(flash_call(image, at, 0, 4), 0xa88);
    op(image, 0xa90, &[0xb8], 4);
    op(image, 0xaa0, &[0x8b,0x14,0x85], va(0xab8));
    op(image, 0xaa8, &[0x8b,0x0c,0xb5], va(0xad0));
    image[0xaaf..0xab2].copy_from_slice(&[0x8d, 0x34, 0x86]);
    for (i, value) in [96, 208, 320, 432, 544].into_iter().enumerate() { put32(image, 0xab8 + i * 4, value); }
    for (i, value) in [64, 240, 416, 592].into_iter().enumerate() { put32(image, 0xad0 + i * 4, value); }
    image[0xae0..0xae6].copy_from_slice(&[0x83,0xe9,128,0x83,0xc2,72]);
    op(image,0xaf0,&[0xc7,0x85,0x7c,0xff,0xff,0xff],371);
    op(image,0xb00,&[0xc7,0x45,0x80],640);
    for (i, value) in [
        160,96,480,608,576,560,640,624,656,560,720,624,736,560,800,624,
        816,560,880,624,552,656,664,688,696,656,808,688,824,656,904,688
    ].into_iter().enumerate() { put32(image, 0xb20 + i * 4, value); }
    image[0xba0..0xba5].copy_from_slice(&[0xc1,0xff,5,0x6a,3]);
}
fn add_string(image: &mut [u8], cursor: &mut usize, name: &str) -> u32 {
    let offset = *cursor;
    image[offset..offset + name.len()].copy_from_slice(name.as_bytes());
    *cursor += name.len() + 1;
    va(offset)
}

fn original(groups: &[Vec<String>]) -> Vec<u8> {
    let mut image = vec![0; 0x1000];
    image[..2].copy_from_slice(b"MZ");
    put32(&mut image, 60, 0x80);
    image[0x80..0x88].copy_from_slice(b"PE\0\0\x4c\x01\x01\0");
    image[0x94] = 0xe0;
    image[0x98..0x9a].copy_from_slice(b"\x0b\x01");
    put32(&mut image, 0x98 + 28, ROOT);
    put32(&mut image, 0x98 + 56, 0x2000);
    put32(&mut image, 0x98 + 60, 0x200);
    image[SECTION..SECTION + 6].copy_from_slice(b".rdata");
    for (offset, value) in [(8, 0xe00), (12, 0x1000), (16, 0xe00), (20, 0x200)] {
        put32(&mut image, SECTION + offset, value);
    }
    put32(&mut image, CG, va(CG_ROWS));
    let mut cursor = DATA;
    let mut row = CG_ROWS;
    for group in groups {
        for (i, name) in group.iter().enumerate() {
            let name = if i == 0 { format!("*{name}") } else { name.clone() };
            let addr = add_string(&mut image, &mut cursor, &name);
            put32(&mut image, row, addr);
            row += 4;
        }
    }
    for (i, name) in ["*Alternate.png", "Base.png(Overlay.png)", "*Last.png"]
        .into_iter()
        .enumerate()
    {
        let addr = add_string(&mut image, &mut cursor, name);
        put32(&mut image, 0xf00 + 4 * i, addr);
    }
    for (i, name) in ["alpha", "beta", "gamma", "delta"].into_iter().enumerate() {
        let addr = add_string(&mut image, &mut cursor, name);
        put32(&mut image, FLASH + 4 * i, addr);
    }
    let track = add_string(&mut image, &mut cursor, "Score.ogg");
    for i in 0..47 {
        put32(&mut image, MUSIC + i * 4, track);
    }
    image[MOVIE..MOVIE + 8].copy_from_slice(b"movie.sc");
    geometry(&mut image);
    image
}

fn write_paz(root: &Path, role: &str, entries: impl IntoIterator<Item = String>) {
    let names: BTreeSet<_> = entries.into_iter().collect();
    let bytes = native::archive_entries(role, &names.iter().map(|n| (n.as_str(), b".end\r\n".as_slice())).collect::<Vec<_>>());
    fs::write(root.join(format!("{role}.paz")), bytes).unwrap();
}

#[test]
fn gallery_recipe_extracts_native_order_and_core_rejects_missing_assets() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let common = root.path().join("common");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&common).unwrap();
    for (name, code) in [
        ("binary", include_str!("../../../../Metadata/patches/common/binary.luau")),
        ("pe", include_str!("../../../../Metadata/patches/common/pe.luau")),
        (
            "musica_gallery_table",
            include_str!("../../../../Metadata/patches/common/musica_gallery_table.luau"),
        ),
        ("perseus_gallery", include_str!("../../../../Metadata/patches/common/perseus_gallery.luau")),
    ] {
        fs::write(common.join(format!("{name}.luau")), code).unwrap();
    }
    let mut groups: Vec<_> = (0..21).map(|i| vec![format!("Picture{i}.png")]).collect();
    groups[20].push("Second.png".into());
    groups[20].push("Third.png".into());
    let image = original(&groups);
    fs::write(source.join("original.exe"), &image).unwrap();

    let profile = format!("schema='shinki.emu.musica.keys.v2'\npaz_version=0\nindex_size_xor=0\nrc4_skip_crc=true\n{}", ["bg", "bgm", "scr", "sys"].map(native::key_role).concat());
    fs::write(source.join("key.toml"), profile).unwrap();
    let mut sys = vec!["memories.png", "memoriesOver.png", "cgmode0.png", "cgmode0box.png", "cgmode0menu.png", "cgpage001.png", "cgpage002.png", "musicPage1.png", "musicPage2.png", "musicPage3.png", "musicPageOver.png", "musicNote.png"].into_iter().map(str::to_owned).collect::<Vec<_>>();
    sys.extend(groups.iter().map(|g| format!("cgthumb\\{}", g[0])));
    for i in 0..4 {
        for suffix in [".png", "Over.png", "menu.png"] {
            sys.push(format!("flash{i}{suffix}"));
        }
    }
    write_paz(&source, "sys", sys);
    let mut scr = vec!["movie.sc".to_owned(), "test.sc".to_owned()];
    for (page, name) in ["alpha", "beta", "gamma", "delta"].into_iter().enumerate() {
        let slots: &[u8] = if page == 0 { &[4, 5, 6, 7, 8, 16] } else { &[4, 5, 6, 12] };
        scr.extend(slots.iter().map(|i| format!("fb_{name}_{i:02}.sc")));
    }
    write_paz(&source, "scr", scr);
    fs::write(source.join("bgm.paz"), native::archive("bgm", "Score.ogg", b"public music")).unwrap();
    write_paz(&source, "bg", groups.iter().flatten().cloned());

    let recipe = root.path().join("patch.luau");
    let run = |hash: &str, output: &str| {
        fs::write(&recipe, format!(r#"
            return {{id='native-gallery-demo', title='Native gallery fixture', version=1, apply=function(ctx)
                local gallery = ctx.load('common/perseus_gallery.luau')
                local layout = gallery.read(ctx, {{executable='original.exe', size={}, sha256='{hash}',
                    cg_pointer_rva=0x1000, cg_rows=23, cg_groups=21, flash_routes_rva=0x1090,
                    music_rva=0x10a0, movie_entry_rva=0x1170,
                    root_buttons={{0x1600,0x1620,0x1640,0x1660,0x1680}},
                    cg_grid_rva=0x16a0, cg_columns_rva=0x1700, cg_slots_rva=0x1710,
                    cg_size_rva=0x1720, cg_page_x_rva=0x1740, cg_menu_rva=0x1750,
                    page_controls_rva=0x1760,
                    flash_first_rva=0x1843, flash_other_rva=0x1800, flash_finish_rva=0x1888,
                    flash_constructor_rva=0x1c00, flash_pages_rva=0x1890,
                    flash_row_table_rva=0x18a0, flash_col_table_rva=0x18a8, flash_size_rva=0x18e0,
                    flash_menu_x_rva=0x18f0, flash_menu_y_rva=0x1900,
                    music_controls_rva=0x1920, music_row_shift_rva=0x19a0, music_pages_rva=0x19a3}})
                local pe = ctx.load('common/pe.luau')
                local native = ctx.load('common/musica_gallery_table.luau')
                local file = pe.parse(ctx, ctx.open('source','original.exe'))
                local alternate = native.groups(ctx, pe, file, 0x1d00, 3, 2)
                assert(alternate[1][1] == 'Alternate.png' and alternate[1][2] == 'Base.png(Overlay.png)')
                assert(alternate[2][1] == 'Last.png')
                for _, name in ipairs({{'key.toml','bg.paz','bgm.paz','scr.paz','sys.paz'}}) do
                    ctx.copy('source',name,name)
                end
                local path, required = gallery.install(ctx, layout)
                local frontend = ctx.json.encode({{gallery_layout=path, titles={{{{
                    background='musica:/sys/memories.png',
                    buttons={{{{rect={{10,20,80,40}},action='memories'}}}}
                }}}}}})
                ctx.musica_validate_title_frontend('key.toml', frontend, 'gbk', 1280, 720)
                return {{plugin_id='shinki.emu.musica', required_capabilities=required, configuration={{
                    launch_mode='title', frontend=frontend}}}}
            end}}
        "#, image.len())).unwrap();
        apply(PatchRequest {
            script: recipe.clone(), source: source.clone(), patch: source.clone(),
            mode: PatchMode::NewDirectory { output: root.path().join(output) },
        }, Arc::new(AtomicBool::new(false)), |_| {})
    };
    let hash = hex::encode(Sha256::digest(&image));
    let result = run(&hash, "valid").unwrap();
    assert_eq!(result.profile.required_capabilities, shinki_emu_abi::CAP_AUTHORED_GALLERY_CLIP);
    let receipt = shinki_emu_patch::read_output_profile(&result.output, &AtomicBool::new(false))
        .unwrap().expect("installed patch receipt");
    assert_eq!(receipt.required_capabilities, result.profile.required_capabilities);
    let config: serde_json::Value = serde_json::from_str(&result.profile.configuration["frontend"]).unwrap();
    let path = config["gallery_layout"].as_str().unwrap();
    let gallery: serde_json::Value = serde_json::from_slice(&fs::read(result.output.join(path)).unwrap()).unwrap();
    let screens = gallery["screens"].as_array().unwrap();
    let cg_two = &screens[2];
    assert_eq!(screens[0]["buttons"][0]["rect"], serde_json::json!([20, 40, 80, 40]));
    assert_eq!(cg_two["id"], "cg2");
    assert_eq!(cg_two["buttons"][0]["action"]["images"], serde_json::json!([
        "musica:/bg/Picture20.png", "musica:/bg/Second.png", "musica:/bg/Third.png"
    ]));
    assert!(cg_two["buttons"][0]["action"]["viewed_only"].is_null());
    assert_eq!(cg_two["layers"][1]["clip"], serde_json::json!([64, 96, 136, 80]));
    assert_eq!(cg_two["buttons"][2]["action"]["screen"], "cg1");
    assert_eq!(screens[3]["buttons"][5]["action"]["resource"], "musica:/scr/fb_alpha_16.sc");
    assert_eq!(screens[9]["buttons"][14]["action"]["resource"], "musica:/bgm/Score.ogg");
    assert_eq!(screens[9]["buttons"][15]["action"]["kind"], "previous_track");
    assert!(!result.output.join("original.exe").exists());

    write_paz(&source, "bg", groups.iter().flatten().filter(|name| name.as_str() != "Second.png").cloned());
    assert_eq!(run(&hash, "missing").err().unwrap().kind(), PatchErrorKind::Integrity);
    assert!(!root.path().join("missing").exists());
    assert_eq!(run(&"0".repeat(64), "wrong").err().unwrap().kind(), PatchErrorKind::UnsupportedInput);
    assert!(!root.path().join("wrong").exists());
    let mut unknown = image.clone();
    unknown[0x800] = 0x90;
    fs::write(source.join("original.exe"), &unknown).unwrap();
    let unknown_hash = hex::encode(Sha256::digest(unknown));
    assert_eq!(run(&unknown_hash, "unknown").err().unwrap().kind(), PatchErrorKind::UnsupportedInput);
    assert!(!root.path().join("unknown").exists());
    let mut unbacked = image.clone();
    put32(&mut unbacked, CG, ROOT + 0x2000);
    fs::write(source.join("original.exe"), &unbacked).unwrap();
    let unbacked_hash = hex::encode(Sha256::digest(unbacked));
    assert_eq!(run(&unbacked_hash, "out-of-range").err().unwrap().kind(), PatchErrorKind::Bounds);
    assert!(!root.path().join("out-of-range").exists());
    let mut truncated = image.clone();
    put32(&mut truncated, CG_ROWS + 4, 0);
    fs::write(source.join("original.exe"), &truncated).unwrap();
    let truncated_hash = hex::encode(Sha256::digest(truncated));
    assert_eq!(
        run(&truncated_hash, "truncated").err().unwrap().kind(),
        PatchErrorKind::UnsupportedInput
    );
    assert!(!root.path().join("truncated").exists());
}
