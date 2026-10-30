#include <ace/xcomponent/native_interface_xcomponent.h>
#include <napi/native_api.h>
#include <native_window/external_window.h>
#include <native_buffer/native_buffer.h>
#include <native_vsync/native_vsync.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

extern void shinki_ohos_event(int kind, double x, double y, const char *text);
extern void shinki_ohos_start(const char *files);
static pthread_mutex_t surface_lock = PTHREAD_MUTEX_INITIALIZER;
static OHNativeWindow *surface;
static OH_NativeVSync *vsync;
static bool foreground = true;
static void frame(long long timestamp, void *data) {
    (void)timestamp; (void)data;
    shinki_ohos_event(0, 0, 0, NULL);
    pthread_mutex_lock(&surface_lock);
    if (surface && foreground) OH_NativeVSync_RequestFrame(vsync, frame, NULL);
    pthread_mutex_unlock(&surface_lock);
}
static void changed(OH_NativeXComponent *component, void *window) {
    uint64_t width = 0, height = 0;
    if (OH_NativeXComponent_GetXComponentSize(component, window, &width, &height) == 0)
        shinki_ohos_event(1, (double)width, (double)height, NULL);
}
static void created(OH_NativeXComponent *component, void *window) {
    pthread_mutex_lock(&surface_lock);
    if (surface) OH_NativeWindow_NativeObjectUnreference(surface);
    surface = window;
    OH_NativeWindow_NativeObjectReference(surface);
    OH_NativeWindow_NativeWindowHandleOpt(surface, SET_FORMAT, NATIVEBUFFER_PIXEL_FMT_RGBA_8888);
    OH_NativeWindow_NativeWindowHandleOpt(surface, SET_USAGE, (uint64_t)(NATIVEBUFFER_USAGE_CPU_WRITE | NATIVEBUFFER_USAGE_CPU_READ | NATIVEBUFFER_USAGE_HW_TEXTURE));
    if (!vsync) vsync = OH_NativeVSync_Create("ShinkiEMU", 9);
    if (vsync && foreground) OH_NativeVSync_RequestFrame(vsync, frame, NULL);
    pthread_mutex_unlock(&surface_lock);
    changed(component, window);
    shinki_ohos_event(5, 1, 0, NULL);
}
static void destroyed(OH_NativeXComponent *component, void *window) {
    (void)component; (void)window;
    pthread_mutex_lock(&surface_lock);
    if (surface) OH_NativeWindow_NativeObjectUnreference(surface);
    surface = NULL;
    pthread_mutex_unlock(&surface_lock);
    shinki_ohos_event(5, 0, 0, NULL);
}
static void touch(OH_NativeXComponent *component, void *window) {
    OH_NativeXComponent_TouchEvent event;
    if (OH_NativeXComponent_GetTouchEvent(component, window, &event) == 0)
        shinki_ohos_event(2, event.x, event.y, event.type == OH_NATIVEXCOMPONENT_DOWN ? "down" : event.type == OH_NATIVEXCOMPONENT_UP ? "up" : event.type == OH_NATIVEXCOMPONENT_CANCEL ? "cancel" : "move");
}
static void key(OH_NativeXComponent *component, void *window) {
    (void)window;
    OH_NativeXComponent_KeyEvent *event = NULL;
    OH_NativeXComponent_KeyCode code;
    OH_NativeXComponent_KeyAction action;
    if (OH_NativeXComponent_GetKeyEvent(component, &event) == 0 &&
        OH_NativeXComponent_GetKeyEventCode(event, &code) == 0 &&
        OH_NativeXComponent_GetKeyEventAction(event, &action) == 0)
        shinki_ohos_event(3, code, action, NULL);
}
int shinki_ohos_present(void (*draw)(void *, unsigned char *, int, int, int), void *data) {
    int result = 0;
    pthread_mutex_lock(&surface_lock);
    if (!surface || !foreground) { pthread_mutex_unlock(&surface_lock); return 0; }
    OHNativeWindowBuffer *buffer = NULL;
    int fence = -1;
    result = OH_NativeWindow_NativeWindowRequestBuffer(surface, &buffer, &fence);
    if (!result) {
        if (fence >= 0) { struct pollfd p = { fence, POLLIN, 0 }; if (poll(&p, 1, 1000) <= 0) result = -1; close(fence); }
        OH_NativeBuffer *native = NULL;
        void *pixels = NULL;
        if (!result) result = OH_NativeBuffer_FromNativeWindowBuffer(buffer, &native);
        if (!result) result = OH_NativeBuffer_Map(native, &pixels);
        if (!result) {
            OH_NativeBuffer_Config config;
            OH_NativeBuffer_GetConfig(native, &config);
            draw(data, pixels, config.width, config.height, config.stride);
            result = OH_NativeBuffer_Unmap(native);
        }
        if (!result) { Region region = {0}; result = OH_NativeWindow_NativeWindowFlushBuffer(surface, buffer, -1, region); }
        else OH_NativeWindow_NativeWindowAbortBuffer(surface, buffer);
    }
    pthread_mutex_unlock(&surface_lock);
    return result;
}
static bool string_arg(napi_env env, napi_callback_info info, char *buffer, size_t capacity) {
    napi_value arg; size_t count = 1, written = 0;
    return napi_get_cb_info(env, info, &count, &arg, NULL, NULL) == napi_ok && count == 1 &&
        napi_get_value_string_utf8(env, arg, buffer, capacity, &written) == napi_ok && written < capacity;
}
static napi_value start(napi_env env, napi_callback_info info) {
    char path[4096];
    if (string_arg(env, info, path, sizeof(path))) shinki_ohos_start(path);
    return NULL;
}
static napi_value text(napi_env env, napi_callback_info info) {
    char value[4096];
    if (string_arg(env, info, value, sizeof(value))) shinki_ohos_event(4, 0, 0, value);
    return NULL;
}
static napi_value lifecycle(napi_env env, napi_callback_info info) {
    size_t count = 2; napi_value args[2]; bool active; double dpi;
    if (napi_get_cb_info(env, info, &count, args, NULL, NULL) == napi_ok && count == 2 &&
        napi_get_value_bool(env, args[0], &active) == napi_ok && napi_get_value_double(env, args[1], &dpi) == napi_ok) {
        pthread_mutex_lock(&surface_lock);
        foreground = active;
        if (surface && active && vsync) OH_NativeVSync_RequestFrame(vsync, frame, NULL);
        pthread_mutex_unlock(&surface_lock);
        shinki_ohos_event(6, active, dpi, NULL);
    }
    return NULL;
}
typedef struct { char *kind; char *payload; } OhosMessage;
static napi_threadsafe_function event_listener;
extern void shinki_ohos_pick_result(uint32_t id, const char *path, const char *error);
static void call_js(napi_env env, napi_value callback, void *context, void *data) {
    (void)context;
    OhosMessage *message = data;
    if (env && callback) {
        napi_value global, kind, payload, args[2];
        napi_get_global(env, &global);
        napi_create_string_utf8(env, message->kind, NAPI_AUTO_LENGTH, &kind);
        napi_create_string_utf8(env, message->payload, NAPI_AUTO_LENGTH, &payload);
        args[0] = kind; args[1] = payload;
        napi_call_function(env, global, callback, 2, args, NULL);
    }
    free(message->kind); free(message->payload); free(message);
}
int shinki_ohos_notify(const char *kind, const char *payload) {
    if (!event_listener) return -1;
    OhosMessage *message = calloc(1, sizeof(*message));
    if (!message) return -1;
    message->kind = strdup(kind); message->payload = strdup(payload);
    if (!message->kind || !message->payload || napi_call_threadsafe_function(event_listener, message, napi_tsfn_nonblocking) != napi_ok) {
        free(message->kind); free(message->payload); free(message); return -1;
    }
    return 0;
}
static napi_value set_event_listener(napi_env env, napi_callback_info info) {
    napi_value callback, resource_name;
    size_t count = 1;
    if (napi_get_cb_info(env, info, &count, &callback, NULL, NULL) != napi_ok || count != 1) return NULL;
    napi_create_string_utf8(env, "ShinkiEMU ArkTS bridge", NAPI_AUTO_LENGTH, &resource_name);
    if (event_listener) napi_release_threadsafe_function(event_listener, napi_tsfn_release);
    napi_create_threadsafe_function(env, callback, NULL, resource_name, 0, 1, NULL, NULL, NULL, call_js, &event_listener);
    return NULL;
}
static napi_value complete_picker(napi_env env, napi_callback_info info) {
    size_t count = 3; napi_value args[3]; double id; char path[4096], error[512] = {0};
    if (napi_get_cb_info(env, info, &count, args, NULL, NULL) == napi_ok && count == 3 &&
        napi_get_value_double(env, args[0], &id) == napi_ok &&
        napi_get_value_string_utf8(env, args[1], path, sizeof(path), NULL) == napi_ok &&
        napi_get_value_string_utf8(env, args[2], error, sizeof(error), NULL) == napi_ok)
        shinki_ohos_pick_result((uint32_t)id, path, error);
    return NULL;
}
static napi_value picker(napi_env env, napi_callback_info info) {
    size_t count = 2; napi_value args[2]; char kind[64], payload[4096];
    if (napi_get_cb_info(env, info, &count, args, NULL, NULL) == napi_ok && count == 2 &&
        napi_get_value_string_utf8(env, args[0], kind, sizeof(kind), NULL) == napi_ok &&
        napi_get_value_string_utf8(env, args[1], payload, sizeof(payload), NULL) == napi_ok)
        shinki_ohos_notify(kind, payload);
    return NULL;
}
static OH_NativeXComponent_Callback callbacks = { created, changed, destroyed, touch };
static napi_value initialize(napi_env env, napi_value exports) {
    napi_value value; OH_NativeXComponent *component = NULL;
    if (napi_get_named_property(env, exports, OH_NATIVE_XCOMPONENT_OBJ, &value) == napi_ok && napi_unwrap(env, value, (void **)&component) == napi_ok && component) {
        OH_NativeXComponent_RegisterCallback(component, &callbacks);
        OH_NativeXComponent_RegisterKeyEventCallback(component, key);
    }
    napi_property_descriptor methods[] = {
        { "start", NULL, start, NULL, NULL, NULL, napi_default, NULL },
        { "text", NULL, text, NULL, NULL, NULL, napi_default, NULL },
        { "lifecycle", NULL, lifecycle, NULL, NULL, NULL, napi_default, NULL },
        { "setEventListener", NULL, set_event_listener, NULL, NULL, NULL, napi_default, NULL },
        { "completePicker", NULL, complete_picker, NULL, NULL, NULL, napi_default, NULL },
        { "picker", NULL, picker, NULL, NULL, NULL, napi_default, NULL }
    };
    napi_define_properties(env, exports, 6, methods);
    return exports;
}
static napi_module module = { 1, 0, NULL, initialize, "shinki_emu_manager", NULL, {0} };
__attribute__((constructor)) static void register_module(void) { napi_module_register(&module); }
