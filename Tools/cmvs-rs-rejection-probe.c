/* Native Windows ABI behavior probe. Opens only releases expected to be rejected;
 * no game loop, rendering, screenshots or registry overrides are implemented here.
 * Usage: cmvs-rs-rejection-probe.exe <packaged DLL> <game root> [game root ...]
 */
#include "shinki_emu.h"
#include <windows.h>
#include <stdio.h>
#include <string.h>

static void emit(void *context, const ShinkiEmuLogRecord *record) {
    (void)context;
    (void)record;
}
static void flush(void *context) { (void)context; }
static uint32_t not_cancelled(void *context, uint64_t session) {
    (void)context;
    (void)session;
    return 0;
}
int wmain(int argc, wchar_t **argv) {
    if (argc < 3) return 2;
    HMODULE library = LoadLibraryExW(argv[1], NULL,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!library) {
        fprintf(stderr, "LoadLibraryExW failed: %lu\n", GetLastError());
        return 3;
    }
    ShinkiEmuQuery query = (ShinkiEmuQuery)GetProcAddress(library, "shinki_emu_query");
    const ShinkiEmuCoreApi *api = NULL;
    if (!query || query(ShinkiEmuABI_VERSION, &api) != ShinkiEmuOK || !api) return 4;
    ShinkiEmuDiagnostics diagnostics = {0};
    diagnostics.header.size = sizeof diagnostics;
    diagnostics.header.version = ShinkiEmuABI_VERSION;
    diagnostics.emit = emit;
    diagnostics.flush = flush;
    ShinkiEmuError error = {0};
    if (api->initialize(&diagnostics, &error) != ShinkiEmuOK) return 5;
    ShinkiEmuDescriptor descriptor = {0};
    if (api->describe(&descriptor, &error) != ShinkiEmuOK ||
        descriptor.plugin_id.len != 7 || memcmp(descriptor.plugin_id.data, "cmvs-rs", 7)) return 6;
    for (int sample = 2; sample < argc; ++sample) {
        char path[32768];
        if (!WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, argv[sample], -1,
            path, sizeof path, NULL, NULL)) return 7;
        ShinkiEmuBytes root = {(const uint8_t *)path, strlen(path)};
        for (int attempt = 0; attempt < 2; ++attempt) {
            ShinkiEmuProbeResult detected = {0};
            ShinkiEmuStatus probe_status = api->probe(root, &detected, &error);
            if (probe_status != ShinkiEmuOK || !detected.game_id.len || !detected.format.len) return 8;
            ShinkiEmuOpenRequest request = {0};
            request.header.size = sizeof request;
            request.header.version = ShinkiEmuABI_VERSION;
            request.game_path = root;
            request.session_epoch = (uint64_t)(sample * 2 + attempt);
            request.window.drawable_width = 1280;
            request.window.drawable_height = 720;
            request.window.scale_factor = 1.0;
            request.window.focused = 1;
            request.host.header.size = sizeof request.host;
            request.host.header.version = ShinkiEmuABI_VERSION;
            request.host.cancellation.is_cancelled = not_cancelled;
            ShinkiEmuOpenResult opened = {0};
            ShinkiEmuStatus status = api->open(&request, &opened, &error);
            if (status != ShinkiEmuNO_MATCH || opened.session != NULL) {
                fprintf(stderr, "sample=%d attempt=%d probe=%d open=%d session_null=%d\n",
                    sample - 2, attempt, probe_status, status, opened.session == NULL);
                if (opened.session) {
                    api->cancel(opened.session);
                    api->close(opened.session, &error);
                }
                return 9;
            }
            printf("{\"sample\":%d,\"attempt\":%d,\"probe_status\":%d,\"open_status\":%d,"
                   "\"session_null\":true,\"diagnostic\":\"%.*s\"}\n",
                sample - 2, attempt, probe_status, status, (int)error.code_len, error.code);
        }
    }
    FreeLibrary(library);
    return 0;
}
