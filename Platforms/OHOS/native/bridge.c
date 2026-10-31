// NAPI module, XComponent surface/input glue and vsync pacing for the
// ShinkiEMU Manager. The Rust side (platform/ohos.rs) owns all state beyond
// the native window, the vsync handle and the ArkTS event listener.
#include <ace/xcomponent/native_interface_xcomponent.h>
#include <hilog/log.h>
#include <napi/native_api.h>
#include <native_buffer/native_buffer.h>
#include <native_vsync/native_vsync.h>
#include <native_window/external_window.h>
#include <poll.h>
#include <pthread.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#undef LOG_DOMAIN
#undef LOG_TAG
#define LOG_DOMAIN 0xD001
#define LOG_TAG "ShinkiEMU"

enum {
    EVENT_FRAME = 0,
    EVENT_RESIZE = 1,
    EVENT_TOUCH = 2,
    EVENT_KEY = 3,
    EVENT_SURFACE = 5,
    EVENT_FOREGROUND = 6,
    EVENT_CONFIGURATION = 7,
};

extern void shinki_ohos_event(int kind, double x, double y, const char *text);
extern void shinki_ohos_start(const char *files);
extern void shinki_ohos_pick_result(uint32_t id, const char *path, const char *error);

static pthread_mutex_t surface_lock = PTHREAD_MUTEX_INITIALIZER;
static OHNativeWindow *surface;
static OH_NativeVSync *vsync;
static bool frame_pending;
static napi_threadsafe_function event_listener;

static void frame(long long timestamp, void *data) {
    (void)timestamp;
    (void)data;
    pthread_mutex_lock(&surface_lock);
    frame_pending = false;
    pthread_mutex_unlock(&surface_lock);
    shinki_ohos_event(EVENT_FRAME, 0, 0, NULL);
}

// Called by the Manager thread whenever Slint may need to draw.
void shinki_ohos_request_frame(void) {
    pthread_mutex_lock(&surface_lock);
    if (surface && vsync && !frame_pending) {
        frame_pending = OH_NativeVSync_RequestFrame(vsync, frame, NULL) == 0;
    }
    pthread_mutex_unlock(&surface_lock);
}

static void resize(OH_NativeXComponent *component, void *window) {
    uint64_t width = 0, height = 0;
    if (OH_NativeXComponent_GetXComponentSize(component, window, &width, &height) != 0) return;
    pthread_mutex_lock(&surface_lock);
    if (surface == window) {
        OH_NativeWindow_NativeWindowHandleOpt(surface, SET_BUFFER_GEOMETRY, (int32_t)width, (int32_t)height);
    }
    pthread_mutex_unlock(&surface_lock);
    shinki_ohos_event(EVENT_RESIZE, (double)width, (double)height, NULL);
}

static void surface_created(OH_NativeXComponent *component, void *window) {
    pthread_mutex_lock(&surface_lock);
    if (surface) OH_NativeWindow_NativeObjectUnreference(surface);
    surface = window;
    OH_NativeWindow_NativeObjectReference(surface);
    OH_NativeWindow_NativeWindowHandleOpt(surface, SET_FORMAT, NATIVEBUFFER_PIXEL_FMT_RGBA_8888);
    OH_NativeWindow_NativeWindowHandleOpt(surface, SET_USAGE,
        (uint64_t)(NATIVEBUFFER_USAGE_CPU_READ | NATIVEBUFFER_USAGE_CPU_WRITE | NATIVEBUFFER_USAGE_MEM_DMA));
    if (!vsync) {
        static const char name[] = "ShinkiEMU";
        vsync = OH_NativeVSync_Create(name, sizeof(name) - 1);
        if (!vsync) OH_LOG_ERROR(LOG_APP, "OH_NativeVSync_Create failed");
    }
    pthread_mutex_unlock(&surface_lock);
    resize(component, window);
    shinki_ohos_event(EVENT_SURFACE, 1, 0, NULL);
}

static void surface_changed(OH_NativeXComponent *component, void *window) {
    resize(component, window);
}

static void surface_destroyed(OH_NativeXComponent *component, void *window) {
    (void)component;
    pthread_mutex_lock(&surface_lock);
    if (surface == window) {
        OH_NativeWindow_NativeObjectUnreference(surface);
        surface = NULL;
    }
    pthread_mutex_unlock(&surface_lock);
    shinki_ohos_event(EVENT_SURFACE, 0, 0, NULL);
}

static void touch(OH_NativeXComponent *component, void *window) {
    OH_NativeXComponent_TouchEvent event;
    if (OH_NativeXComponent_GetTouchEvent(component, window, &event) != 0) return;
    const char *phase = event.type == OH_NATIVEXCOMPONENT_DOWN ? "down"
        : event.type == OH_NATIVEXCOMPONENT_UP ? "up"
        : event.type == OH_NATIVEXCOMPONENT_CANCEL ? "cancel"
        : "move";
    shinki_ohos_event(EVENT_TOUCH, event.x, event.y, phase);
}

static void mouse(OH_NativeXComponent *component, void *window) {
    OH_NativeXComponent_MouseEvent event;
    if (OH_NativeXComponent_GetMouseEvent(component, window, &event) != 0) return;
    const char *phase;
    switch (event.action) {
    case OH_NATIVEXCOMPONENT_MOUSE_PRESS:
        if (event.button != OH_NATIVEXCOMPONENT_LEFT_BUTTON) return;
        phase = "down";
        break;
    case OH_NATIVEXCOMPONENT_MOUSE_RELEASE:
        if (event.button != OH_NATIVEXCOMPONENT_LEFT_BUTTON) return;
        phase = "up";
        break;
    case OH_NATIVEXCOMPONENT_MOUSE_MOVE:
        phase = "move";
        break;
    default:
        return;
    }
    shinki_ohos_event(EVENT_TOUCH, event.x, event.y, phase);
}

static void hover(OH_NativeXComponent *component, bool hovering) {
    (void)component;
    if (!hovering) shinki_ohos_event(EVENT_TOUCH, 0, 0, "cancel");
}

static void key(OH_NativeXComponent *component, void *window) {
    (void)window;
    OH_NativeXComponent_KeyEvent *event = NULL;
    OH_NativeXComponent_KeyCode code;
    OH_NativeXComponent_KeyAction action;
    if (OH_NativeXComponent_GetKeyEvent(component, &event) == 0 &&
        OH_NativeXComponent_GetKeyEventCode(event, &code) == 0 &&
        OH_NativeXComponent_GetKeyEventAction(event, &action) == 0) {
        shinki_ohos_event(EVENT_KEY, code, action == OH_NATIVEXCOMPONENT_KEY_ACTION_DOWN ? 0 : 1, NULL);
    }
}

// Maps the next window buffer, lets `draw` fill it, and queues it. Returns 0
// on success or when no surface exists.
int shinki_ohos_present(void (*draw)(void *, unsigned char *, int, int, int), void *data) {
    pthread_mutex_lock(&surface_lock);
    if (!surface) {
        pthread_mutex_unlock(&surface_lock);
        return 0;
    }
    OHNativeWindowBuffer *buffer = NULL;
    int fence = -1;
    int result = OH_NativeWindow_NativeWindowRequestBuffer(surface, &buffer, &fence);
    if (result == 0) {
        if (fence >= 0) {
            struct pollfd wait = { fence, POLLIN, 0 };
            if (poll(&wait, 1, 3000) <= 0) result = -1;
            close(fence);
        }
        OH_NativeBuffer *native = NULL;
        void *pixels = NULL;
        if (result == 0) result = OH_NativeBuffer_FromNativeWindowBuffer(buffer, &native);
        if (result == 0) result = OH_NativeBuffer_Map(native, &pixels);
        if (result == 0) {
            OH_NativeBuffer_Config config;
            OH_NativeBuffer_GetConfig(native, &config);
            draw(data, pixels, config.width, config.height, config.stride);
            result = OH_NativeBuffer_Unmap(native);
        }
        if (result == 0) {
            Region region = { NULL, 0 };
            result = OH_NativeWindow_NativeWindowFlushBuffer(surface, buffer, -1, region);
        } else {
            OH_NativeWindow_NativeWindowAbortBuffer(surface, buffer);
        }
    }
    pthread_mutex_unlock(&surface_lock);
    if (result != 0) OH_LOG_ERROR(LOG_APP, "present failed: %{public}d", result);
    return result;
}

typedef struct {
    char *kind;
    char *payload;
} Message;

static void call_js(napi_env env, napi_value callback, void *context, void *data) {
    (void)context;
    Message *message = data;
    if (env && callback) {
        napi_value global, args[2];
        napi_get_global(env, &global);
        napi_create_string_utf8(env, message->kind, NAPI_AUTO_LENGTH, &args[0]);
        napi_create_string_utf8(env, message->payload, NAPI_AUTO_LENGTH, &args[1]);
        napi_call_function(env, global, callback, 2, args, NULL);
    }
    free(message->kind);
    free(message->payload);
    free(message);
}

// Queues `kind`/`payload` for the ArkTS listener. Returns 0 when queued.
int shinki_ohos_notify(const char *kind, const char *payload) {
    if (!event_listener) return -1;
    Message *message = calloc(1, sizeof(*message));
    if (!message) return -1;
    message->kind = strdup(kind);
    message->payload = strdup(payload);
    if (!message->kind || !message->payload ||
        napi_call_threadsafe_function(event_listener, message, napi_tsfn_nonblocking) != napi_ok) {
        free(message->kind);
        free(message->payload);
        free(message);
        return -1;
    }
    return 0;
}

static bool arguments(napi_env env, napi_callback_info info, size_t expected, napi_value *values) {
    size_t count = expected;
    return napi_get_cb_info(env, info, &count, values, NULL, NULL) == napi_ok && count == expected;
}

static char *string_value(napi_env env, napi_value value) {
    size_t length = 0;
    if (napi_get_value_string_utf8(env, value, NULL, 0, &length) != napi_ok) return NULL;
    char *text = malloc(length + 1);
    if (text && napi_get_value_string_utf8(env, value, text, length + 1, &length) != napi_ok) {
        free(text);
        return NULL;
    }
    return text;
}

static napi_value start(napi_env env, napi_callback_info info) {
    napi_value value;
    if (!arguments(env, info, 1, &value)) return NULL;
    char *files = string_value(env, value);
    if (files) shinki_ohos_start(files);
    free(files);
    return NULL;
}

static napi_value foreground(napi_env env, napi_callback_info info) {
    napi_value value;
    bool active;
    if (arguments(env, info, 1, &value) && napi_get_value_bool(env, value, &active) == napi_ok) {
        shinki_ohos_event(EVENT_FOREGROUND, active ? 1 : 0, 0, NULL);
    }
    return NULL;
}

static napi_value configure(napi_env env, napi_callback_info info) {
    napi_value values[2];
    bool dark;
    double scale;
    if (arguments(env, info, 2, values) && napi_get_value_bool(env, values[0], &dark) == napi_ok &&
        napi_get_value_double(env, values[1], &scale) == napi_ok) {
        shinki_ohos_event(EVENT_CONFIGURATION, dark ? 1 : 0, scale, NULL);
    }
    return NULL;
}

static napi_value press_key(napi_env env, napi_callback_info info) {
    napi_value values[2];
    int32_t code;
    bool pressed;
    if (arguments(env, info, 2, values) && napi_get_value_int32(env, values[0], &code) == napi_ok &&
        napi_get_value_bool(env, values[1], &pressed) == napi_ok) {
        shinki_ohos_event(EVENT_KEY, code, pressed ? 0 : 1, NULL);
    }
    return NULL;
}

static napi_value set_event_listener(napi_env env, napi_callback_info info) {
    napi_value callback, name;
    if (!arguments(env, info, 1, &callback)) return NULL;
    napi_create_string_utf8(env, "ShinkiEMU Manager events", NAPI_AUTO_LENGTH, &name);
    napi_threadsafe_function created = NULL;
    if (napi_create_threadsafe_function(env, callback, NULL, name, 0, 1, NULL, NULL, NULL, call_js, &created) != napi_ok) {
        OH_LOG_ERROR(LOG_APP, "napi_create_threadsafe_function failed");
        return NULL;
    }
    if (event_listener) napi_release_threadsafe_function(event_listener, napi_tsfn_release);
    event_listener = created;
    return NULL;
}

static napi_value complete_picker(napi_env env, napi_callback_info info) {
    napi_value values[3];
    uint32_t id;
    if (!arguments(env, info, 3, values) || napi_get_value_uint32(env, values[0], &id) != napi_ok) return NULL;
    char *path = string_value(env, values[1]);
    char *error = string_value(env, values[2]);
    if (path && error) shinki_ohos_pick_result(id, path, error);
    free(path);
    free(error);
    return NULL;
}

static OH_NativeXComponent_Callback surface_callbacks = {
    .OnSurfaceCreated = surface_created,
    .OnSurfaceChanged = surface_changed,
    .OnSurfaceDestroyed = surface_destroyed,
    .DispatchTouchEvent = touch,
};

static OH_NativeXComponent_MouseEvent_Callback mouse_callbacks = {
    .DispatchMouseEvent = mouse,
    .DispatchHoverEvent = hover,
};

static napi_value initialize(napi_env env, napi_value exports) {
    napi_value object;
    OH_NativeXComponent *component = NULL;
    // XComponent loads the module with its native handle; ArkTS imports load it without.
    if (napi_get_named_property(env, exports, OH_NATIVE_XCOMPONENT_OBJ, &object) == napi_ok &&
        napi_unwrap(env, object, (void **)&component) == napi_ok && component) {
        OH_NativeXComponent_RegisterCallback(component, &surface_callbacks);
        OH_NativeXComponent_RegisterMouseEventCallback(component, &mouse_callbacks);
        OH_NativeXComponent_RegisterKeyEventCallback(component, key);
    }
    napi_property_descriptor methods[] = {
        { "start", NULL, start, NULL, NULL, NULL, napi_default, NULL },
        { "foreground", NULL, foreground, NULL, NULL, NULL, napi_default, NULL },
        { "configure", NULL, configure, NULL, NULL, NULL, napi_default, NULL },
        { "pressKey", NULL, press_key, NULL, NULL, NULL, napi_default, NULL },
        { "setEventListener", NULL, set_event_listener, NULL, NULL, NULL, napi_default, NULL },
        { "completePicker", NULL, complete_picker, NULL, NULL, NULL, napi_default, NULL },
    };
    napi_define_properties(env, exports, sizeof(methods) / sizeof(methods[0]), methods);
    return exports;
}

static napi_module module = {
    .nm_version = 1,
    .nm_flags = 0,
    .nm_filename = NULL,
    .nm_register_func = initialize,
    .nm_modname = "shinki_emu_manager",
    .nm_priv = NULL,
};

__attribute__((constructor)) static void register_module(void) {
    napi_module_register(&module);
}
