# Shinki Lucifen

Install the standalone core ZIP in Manager and select `shinki.emu.lucifen` for
an imported game directory. Point Manager at the root containing `SCRIPT.LPK`,
the companion `.LPK` archives, and the matching Japanese game executable. The
core reads build identifiers and save signatures from the executable; it does
not run the executable. Scanning checks the `SCRIPT.LPK` header and index range.
The core checks the encrypted index and startup modules when it opens the game.

Keep the default `cp932` setting for original Japanese archives. `gbk` is for
translated text already repacked into the matching `.LPK` archives. Changing the
encoding does not make the core load loose `SCRIPT_CHS` files or Windows patch
DLLs. Games and translation data are not distributed with the core.

Use a private writable game copy when testing. Save and load from the game's
own menu, not a Manager save-state command. The save directory comes from
`LL_APP.SYS`; for save mode 2 the core writes to `.shinki/lucifen/saves` beneath
the imported root. Slot previews use the scene selected by the game, without
the save menu overlay. Keep the same imported root when starting a new
session to load a slot. To verify native playback and shutdown, use the default
audio device, leave through the game's own Quit control, and confirm that audio
stops and Manager can reopen the game.

Source builds require the two Noto Sans TrueType fonts in `Assets/Fonts` from
Git LFS. Materialize those files before building the core.

The ARM64 OpenHarmony HAP includes Lucifen as a separate dynamic core. It uses
the same CPU frames and OFL Noto Sans fonts and plays the native 48 kHz stereo
mix through OHAudio. The diagnostic null device remains a non-OpenHarmony option.
Sound position and natural completion follow presented PCM. Normal game exit
drains the submitted tail; cancellation wakes a blocked write or drain and
does not wait for playback. Pausing stops the game and new submissions while
queued PCM plays out. The current audio session ends on a renderer error,
presentation-clock reset, or stalled device.

The standalone package includes the [MPL-2.0 source links](../../../docs/licenses/shinki-emu-lucifen/SOURCE.md),
AWS-LC attributions, dependency licenses, and a `source-commit.txt` stamp.
