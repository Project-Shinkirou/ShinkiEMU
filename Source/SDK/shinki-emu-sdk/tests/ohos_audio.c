/* Host fixture (run from the SDK crate):
   gcc -std=gnu11 -pthread '-D__availability__(...)=' \
     -idirafter "$OHOS_SDK_NATIVE/sysroot/usr/include" tests/ohos_audio.c -o audio-test
   The test drives only the renderer callbacks and presentation position.
   Queue, clock, drain and cancellation all use the production code. */
#include <assert.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <pthread.h>
#include <time.h>

#ifdef _WIN32
// winpthreads only supports realtime waits. Translate the monotonic deadline
// immediately before the real timed wait; keep the same mutex and condition.
static int use_monotonic(pthread_condattr_t *attr, clockid_t clock) {
    (void)attr;
    assert(clock == CLOCK_MONOTONIC);
    return 0;
}
#define pthread_condattr_setclock use_monotonic
#endif

static atomic_uint waits;

static int observe_wait(pthread_cond_t *cond, pthread_mutex_t *mutex, const struct timespec *until) {
    atomic_fetch_add(&waits, 1);
#ifdef _WIN32
    struct timespec monotonic, realtime;
    clock_gettime(CLOCK_MONOTONIC, &monotonic);
    clock_gettime(CLOCK_REALTIME, &realtime);
    int64_t left = ((int64_t)until->tv_sec - monotonic.tv_sec) * 1000000000
        + until->tv_nsec - monotonic.tv_nsec;
    if (left > 0) {
        realtime.tv_sec += left / 1000000000;
        realtime.tv_nsec += left % 1000000000;
        if (realtime.tv_nsec >= 1000000000) {
            realtime.tv_sec++;
            realtime.tv_nsec -= 1000000000;
        }
    }
    return pthread_cond_timedwait(cond, mutex, &realtime);
#else
    return pthread_cond_timedwait(cond, mutex, until);
#endif
}

#define pthread_cond_timedwait observe_wait
#include "../src/runtime/audio/ohos.c"
#undef pthread_cond_timedwait
#undef pthread_condattr_setclock

struct OH_AudioRendererStruct {
    OH_AudioRenderer_Callbacks callbacks;
    void *user;
    atomic_llong position;
};

struct OH_AudioStreamBuilderStruct {
    OH_AudioRenderer_Callbacks callbacks;
    void *user;
};

OH_AudioStream_Result OH_AudioStreamBuilder_Create(OH_AudioStreamBuilder **builder, OH_AudioStream_Type type) {
    assert(type == AUDIOSTREAM_TYPE_RENDERER);
    *builder = calloc(1, sizeof(**builder));
    assert(*builder);
    return AUDIOSTREAM_SUCCESS;
}

OH_AudioStream_Result OH_AudioStreamBuilder_Destroy(OH_AudioStreamBuilder *builder) {
    free(builder);
    return AUDIOSTREAM_SUCCESS;
}

OH_AudioStream_Result OH_AudioStreamBuilder_SetSamplingRate(OH_AudioStreamBuilder *builder, int32_t rate) {
    assert(builder && rate == 48000);
    return AUDIOSTREAM_SUCCESS;
}

OH_AudioStream_Result OH_AudioStreamBuilder_SetChannelCount(OH_AudioStreamBuilder *builder, int32_t channels) {
    assert(builder && channels == 2);
    return AUDIOSTREAM_SUCCESS;
}

OH_AudioStream_Result OH_AudioStreamBuilder_SetSampleFormat(OH_AudioStreamBuilder *builder, OH_AudioStream_SampleFormat format) {
    assert(builder && format == AUDIOSTREAM_SAMPLE_F32LE);
    return AUDIOSTREAM_SUCCESS;
}

OH_AudioStream_Result OH_AudioStreamBuilder_SetRendererInfo(OH_AudioStreamBuilder *builder, OH_AudioStream_Usage usage) {
    assert(builder && usage == AUDIOSTREAM_USAGE_GAME);
    return AUDIOSTREAM_SUCCESS;
}

OH_AudioStream_Result OH_AudioStreamBuilder_SetRendererCallback(OH_AudioStreamBuilder *builder,
    OH_AudioRenderer_Callbacks callbacks, void *user) {
    assert(callbacks.OH_AudioRenderer_OnWriteData && callbacks.OH_AudioRenderer_OnError);
    builder->callbacks = callbacks;
    builder->user = user;
    return AUDIOSTREAM_SUCCESS;
}

OH_AudioStream_Result OH_AudioStreamBuilder_GenerateRenderer(OH_AudioStreamBuilder *builder, OH_AudioRenderer **renderer) {
    OH_AudioRenderer *device = calloc(1, sizeof(*device));
    assert(device);
    device->callbacks = builder->callbacks;
    device->user = builder->user;
    atomic_init(&device->position, 0);
    *renderer = device;
    return AUDIOSTREAM_SUCCESS;
}

OH_AudioStream_Result OH_AudioRenderer_Start(OH_AudioRenderer *renderer) {
    assert(renderer);
    return AUDIOSTREAM_SUCCESS;
}

OH_AudioStream_Result OH_AudioRenderer_Stop(OH_AudioRenderer *renderer) {
    assert(renderer);
    return AUDIOSTREAM_SUCCESS;
}

OH_AudioStream_Result OH_AudioRenderer_Release(OH_AudioRenderer *renderer) {
    free(renderer);
    return AUDIOSTREAM_SUCCESS;
}

OH_AudioStream_Result OH_AudioRenderer_GetAudioTimestampInfo(OH_AudioRenderer *renderer,
    int64_t *position, int64_t *timestamp) {
    *position = atomic_load(&renderer->position);
    *timestamp = *position + 1;
    return AUDIOSTREAM_SUCCESS;
}

static ShinkiAudioStream *open_stream(uint32_t stall_ms) {
    ShinkiAudioStream *stream;
    assert(shinki_ohos_audio_open(&stream, 48000, 2, stall_ms) == 0);
    return stream;
}

static void render(ShinkiAudioStream *stream, float *out, size_t count) {
    OH_AudioRenderer *renderer = stream->renderer;
    assert(renderer->callbacks.OH_AudioRenderer_OnWriteData(renderer, renderer->user,
        out, (int32_t)(count * sizeof(float))) == 0);
}

static uint64_t played(ShinkiAudioStream *stream, uint64_t accepted) {
    AudioProgress progress;
    shinki_ohos_audio_progress(stream, &progress);
    assert(progress.accepted_frames == accepted && progress.played_frames <= accepted);
    return progress.played_frames;
}

struct operation {
    ShinkiAudioStream *stream;
    bool drain;
    int result;
};

static void *run_operation(void *data) {
    struct operation *op = data;
    const float frame[] = {0.1f, 0.2f};
    op->result = op->drain ? shinki_ohos_audio_drain(op->stream)
        : shinki_ohos_audio_write(op->stream, frame, 2);
    return NULL;
}

static pthread_t start_waiter(struct operation *op) {
    pthread_t worker;
    atomic_store(&waits, 0);
    assert(pthread_create(&worker, NULL, run_operation, op) == 0);
    struct timespec deadline = after_ms(300);
    const struct timespec interval = {.tv_nsec = 1000000};
    while (!atomic_load(&waits) && earlier(after_ms(0), deadline)) nanosleep(&interval, NULL);
    assert(atomic_load(&waits));
    return worker;
}

static void finish_waiter(struct operation *op, pthread_t worker, int result) {
    assert(pthread_join(worker, NULL) == 0);
    assert(op->result == result);
}

static void presentation_excludes_underrun_and_drain_waits_for_tail(void) {
    ShinkiAudioStream *stream = open_stream(500);
    const float first[] = {0.1f, 0.2f, 0.3f, 0.4f};
    const float next[] = {0.5f, 0.6f};
    float out[8] = {0};
    render(stream, out, 8); // Four frames of startup silence.
    assert(shinki_ohos_audio_write(stream, first, 4) == 0);
    render(stream, out, 8); // Two source frames followed by two zero frames.
    assert(memcmp(out, first, sizeof(first)) == 0 && out[4] == 0.0f && out[7] == 0.0f);
    assert(shinki_ohos_audio_write(stream, next, 2) == 0);
    render(stream, out, 8); // Last source frame followed by three zero frames.
    assert(memcmp(out, next, sizeof(next)) == 0 && out[2] == 0.0f && out[7] == 0.0f);
    atomic_store(&stream->renderer->position, 4);
    assert(played(stream, 3) == 0);
    atomic_store(&stream->renderer->position, 5);
    assert(played(stream, 3) == 1);
    atomic_store(&stream->renderer->position, 4);
    assert(played(stream, 3) == 1);
    atomic_store(&stream->renderer->position, 8);
    assert(played(stream, 3) == 2);
    struct operation op = {.stream = stream, .drain = true};
    pthread_t worker = start_waiter(&op);
    atomic_store(&stream->renderer->position, 9);
    finish_waiter(&op, worker, 0);
    atomic_store(&stream->renderer->position, 12);
    assert(played(stream, 3) == 3);
    shinki_ohos_audio_close(stream);
}

static void interrupt_wait(bool drain, bool error) {
    ShinkiAudioStream *stream = open_stream(500);
    float pcm[QUEUE_FRAMES * 2] = {0};
    assert(shinki_ohos_audio_write(stream, pcm, drain ? 2 : QUEUE_FRAMES * 2) == 0);
    struct operation op = {.stream = stream, .drain = drain};
    pthread_t worker = start_waiter(&op);
    if (error) {
        OH_AudioRenderer *renderer = stream->renderer;
        renderer->callbacks.OH_AudioRenderer_OnError(renderer, renderer->user,
            AUDIOSTREAM_ERROR_SERVICE_DIED);
    } else {
        shinki_ohos_audio_cancel(stream);
    }
    finish_waiter(&op, worker, error ? AUDIOSTREAM_ERROR_SERVICE_DIED : AUDIO_CANCELLED);
    assert(shinki_ohos_audio_check(stream) == (error ? AUDIOSTREAM_ERROR_SERVICE_DIED : 0));
    shinki_ohos_audio_close(stream);
}

static void clock_stall_surfaces_while_renderer_still_asks_for_pcm(void) {
    ShinkiAudioStream *stream = open_stream(10);
    float frame[] = {0.1f, 0.2f};
    assert(shinki_ohos_audio_write(stream, frame, 2) == 0);
    render(stream, frame, 2);
    const struct timespec stall = {.tv_nsec = 20000000};
    nanosleep(&stall, NULL);
    assert(shinki_ohos_audio_write(stream, frame, 2) == AUDIO_STALLED);
    assert(shinki_ohos_audio_check(stream) == AUDIO_STALLED);
    shinki_ohos_audio_close(stream);
}

int main(void) {
    presentation_excludes_underrun_and_drain_waits_for_tail();
    interrupt_wait(false, false);
    interrupt_wait(true, false);
    interrupt_wait(false, true);
    interrupt_wait(true, true);
    clock_stall_surfaces_while_renderer_still_asks_for_pcm();
    puts("OHAudio presenter, audible drain, cancellation, errors and stall: passed");
}
