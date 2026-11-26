// Synthetic freestanding OHOS DSOs and a Linux x64 query probe. No game input.
#include "shinki_emu.h"

#if defined(FIXTURE_HOST)
#include <assert.h>
#include <dlfcn.h>
#include <stdio.h>
#include <string.h>

int main(int argc, char **argv) {
    assert(argc == 3 || argc == 5);
    for (int i = 1; i < argc; i += 2) {
        void *module = dlopen(argv[i], RTLD_NOW | RTLD_LOCAL);
        if (!module) {
            fprintf(stderr, "%s: %s\n", argv[i], dlerror());
            return 1;
        }
        ShinkiEmuQuery query = (ShinkiEmuQuery)dlsym(module, "shinki_emu_query");
        Dl_info origin;
        assert(query && dladdr((void *)query, &origin));
        assert(strcmp(origin.dli_fname, argv[i]) == 0);
        const ShinkiEmuCoreApi *api = NULL;
        assert(query(ShinkiEmuABI_VERSION, &api) == 0 && api);
        assert(api->header.version == ShinkiEmuABI_VERSION && api->header.size == sizeof(*api));
        assert(api->probe && api->open && api->input && api->control && api->state);
        assert(api->frame && api->cancel && api->close && api->extension);
        ShinkiEmuDescriptor descriptor;
        ShinkiEmuError error = {0};
        const ShinkiEmuDiagnostics diagnostics = {0};
        assert(api->initialize(&diagnostics, &error) == 0);
        assert(api->describe(&descriptor, &error) == 0);
        assert(descriptor.header.version == ShinkiEmuABI_VERSION);
        assert(descriptor.plugin_id.len == strlen(argv[i + 1]));
        assert(memcmp(descriptor.plugin_id.data, argv[i + 1], descriptor.plugin_id.len) == 0);
        printf("%s: ABI v%u, query origin %s, shared dependency resolved\n",
               argv[i + 1], api->header.version, origin.dli_fname);
        assert(query(ShinkiEmuABI_VERSION - 1, &api) == 1 && !api);
    }
}
#elif defined(FIXTURE_DEPENDENCY)
int fixture_dependency(void) { return 42; }
#elif defined(FIXTURE_MANAGER)
extern int fixture_dependency(void);
int fixture_manager(void) { return fixture_dependency(); }
#else
extern int fixture_dependency(void);
#define BYTES(s) {(const uint8_t *)(s), sizeof(s) - 1}

static ShinkiEmuStatus initialize(const ShinkiEmuDiagnostics *diagnostics, ShinkiEmuError *error) {
    return fixture_dependency() == 42 ? 0 : 1;
}
static ShinkiEmuStatus describe(ShinkiEmuDescriptor *out, ShinkiEmuError *error) {
    static const ShinkiEmuBytes formats[] = { BYTES("fixture") };
    static const ShinkiEmuDescriptor descriptor = {
        .header = {sizeof(ShinkiEmuDescriptor), ShinkiEmuABI_VERSION},
        .family_id = BYTES("fixture"), .plugin_id = BYTES(FIXTURE_ID),
        .version = BYTES(FIXTURE_VERSION),
        .capabilities = ShinkiEmuCAP_CPU_SURFACE,
        .formats = formats, .format_count = sizeof(formats) / sizeof(formats[0]),
        .display_name = BYTES("Fixture core"), .family_name = BYTES("Fixture"),
    };
    // Keep the fixture freestanding: neither target should need libc memcpy.
    volatile uint8_t *destination = (volatile uint8_t *)out;
    const uint8_t *source = (const uint8_t *)&descriptor;
    for (size_t i = 0; i < sizeof(descriptor); ++i) {
        destination[i] = source[i];
    }
    return 0;
}
static ShinkiEmuStatus probe(ShinkiEmuBytes path, ShinkiEmuProbeResult *out, ShinkiEmuError *error) { return 1; }
static ShinkiEmuStatus open_core(const ShinkiEmuOpenRequest *request, ShinkiEmuOpenResult *out, ShinkiEmuError *error) { return 1; }
static ShinkiEmuStatus input(ShinkiEmuSession session, const ShinkiEmuInputEvent *events, size_t count, ShinkiEmuError *error) { return 1; }
static ShinkiEmuStatus control(ShinkiEmuSession session, uint32_t command, ShinkiEmuError *error) { return 1; }
static ShinkiEmuStatus state(ShinkiEmuSession session, ShinkiEmuSessionState *out, ShinkiEmuError *error) { return 1; }
static ShinkiEmuStatus frame(ShinkiEmuSession session, ShinkiEmuFrameIdentity id, ShinkiEmuFrameSurface *out, ShinkiEmuError *error) { return 1; }
static void cancel(ShinkiEmuSession session) {}
static ShinkiEmuStatus close_core(ShinkiEmuSession session, ShinkiEmuError *error) { return 1; }
static ShinkiEmuStatus extension(ShinkiEmuBytes name, uint32_t version, const void **out) {
    *out = NULL;
    return 1;
}

static const ShinkiEmuCoreApi api = {
    .header = {sizeof(ShinkiEmuCoreApi), ShinkiEmuABI_VERSION},
    .initialize = initialize, .describe = describe, .probe = probe, .open = open_core,
    .input = input, .control = control, .state = state, .frame = frame,
    .cancel = cancel, .close = close_core, .extension = extension,
};
ShinkiEmuStatus shinki_emu_query(uint32_t version, const ShinkiEmuCoreApi **out) {
    *out = version == ShinkiEmuABI_VERSION ? &api : NULL;
    return *out ? 0 : 1;
}
#endif
