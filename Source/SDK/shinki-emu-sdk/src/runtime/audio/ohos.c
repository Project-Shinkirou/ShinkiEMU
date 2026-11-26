// OHAudio renderer behind the SDK AudioSink. Writers block on a bounded
// interleaved f32 queue; the renderer drains it and pads underruns with silence.
#include <ohaudio/native_audiorenderer.h>
#include <ohaudio/native_audiostreambuilder.h>
#include <errno.h>
#include <pthread.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

// About 100 ms at 48 kHz; bounds latency while absorbing scheduler jitter.
#define QUEUE_FRAMES 4800u

// Contiguous source frames share a span. Only a zero-fill gap creates a new
// span, so this fixed ring easily covers the in-flight device buffers without
// allocating in the callback. A stalled/unusable clock fails rather than
// discarding timing information and reporting a fabricated position.
#define CLOCK_SPANS 256u
#define DRAIN_POLL_MS 5u

enum {
    AUDIO_INTERNAL = -1,
    AUDIO_CANCELLED = -2,
    AUDIO_STALLED = -3,
    AUDIO_CLOCK_OVERFLOW = -4,
    AUDIO_INVALID_PCM = -5,
    AUDIO_CLOCK_RESET = -6
};

typedef struct {
    uint64_t accepted_frames;
    uint64_t played_frames;
} AudioProgress;

typedef struct {
    uint64_t begin;
    uint64_t end;
} AudioSpan;

typedef struct {
    AudioProgress progress;
    uint64_t submitted;
    int64_t device_position;
    int64_t device_timestamp;
    struct timespec deadline;
    AudioSpan spans[CLOCK_SPANS];
    unsigned head;
    unsigned count;
} PlaybackClock;

typedef struct {
    pthread_mutex_t mutex;
    pthread_cond_t changed;
    OH_AudioRenderer *renderer;
    float *samples;
    PlaybackClock *clock;
    uint32_t stall_ms;
    size_t channels;
    size_t capacity;
    size_t read;
    size_t count;
    bool cancelled;
    int failure;
} ShinkiAudioStream;

// Call with the queue mutex held. Preserve the first native/device error.
static void fail(ShinkiAudioStream *stream, int error) {
    if (!stream->failure) stream->failure = error;
    pthread_cond_broadcast(&stream->changed);
}

static int status(const ShinkiAudioStream *stream) {
    return stream->failure ? stream->failure : stream->cancelled ? AUDIO_CANCELLED : 0;
}

static int32_t on_error(OH_AudioRenderer *renderer, void *user_data, OH_AudioStream_Result error) {
    (void)renderer;
    ShinkiAudioStream *stream = user_data;
    pthread_mutex_lock(&stream->mutex);
    fail(stream, error);
    pthread_mutex_unlock(&stream->mutex);
    return 0;
}

static bool record_span(PlaybackClock *clock, size_t wanted, size_t available) {
    uint64_t start = clock->submitted;
    if (available) {
        unsigned last = (clock->head + clock->count + CLOCK_SPANS - 1) % CLOCK_SPANS;
        if (clock->count && clock->spans[last].end == start) {
            clock->spans[last].end += available;
        } else {
            if (clock->count == CLOCK_SPANS) return false;
            unsigned tail = (clock->head + clock->count++) % CLOCK_SPANS;
            clock->spans[tail] = (AudioSpan){start, start + available};
        }
    }
    clock->submitted += wanted;
    return true;
}

// Uses the device's presented frame position, never the queue read count or
// GetFramesWritten. Gaps with no source PCM don't add to the played clock.
static void present(PlaybackClock *clock, uint64_t position) {
    while (clock->count) {
        AudioSpan *span = &clock->spans[clock->head];
        if (position <= span->begin) break;
        uint64_t end = position < span->end ? position : span->end;
        clock->progress.played_frames += end - span->begin;
        span->begin = end;
        if (end < span->end) break;
        clock->head = (clock->head + 1) % CLOCK_SPANS;
        clock->count--;
    }
}

static struct timespec after_ms(uint32_t millis);
static bool earlier(struct timespec a, struct timespec b);

// OHAudio can call back while querying the renderer. Do not hold our mutex
// across the SDK call. Only apply a new timestamp to buffers already known
// before the call.
static void refresh_clock(ShinkiAudioStream *stream) {
    PlaybackClock *clock = stream->clock;
    if (!clock) return;
    pthread_mutex_lock(&stream->mutex);
    uint64_t submitted = clock->submitted;
    bool pending = clock->count && !status(stream);
    pthread_mutex_unlock(&stream->mutex);
    if (!pending) return;

    int64_t position = 0, timestamp = 0;
    OH_AudioStream_Result result = OH_AudioRenderer_GetAudioTimestampInfo(
        stream->renderer, &position, &timestamp);
    pthread_mutex_lock(&stream->mutex);
    if (!status(stream)) {
        uint64_t previous = clock->progress.played_frames;
        if (result == AUDIOSTREAM_SUCCESS && timestamp > clock->device_timestamp) {
            if (position < clock->device_position) {
                // A route may reset the device position. The old and new
                // hardware epochs cannot share one source frame mapping.
                fail(stream, AUDIO_CLOCK_RESET);
            } else {
                clock->device_position = position;
                clock->device_timestamp = timestamp;
                present(clock, (uint64_t)position < submitted ? (uint64_t)position : submitted);
            }
        } else if (result != AUDIOSTREAM_SUCCESS && result != AUDIOSTREAM_ERROR_ILLEGAL_STATE) {
            fail(stream, result);
        }
        if (clock->progress.played_frames != previous) {
            clock->deadline = after_ms(stream->stall_ms);
        } else if (clock->count && !earlier(after_ms(0), clock->deadline)) {
            // The first timestamp may be unavailable. A renderer that keeps
            // asking for PCM but never presents it must still fail the writer.
            fail(stream, AUDIO_STALLED);
        }
    }
    pthread_mutex_unlock(&stream->mutex);
}

static struct timespec after_ms(uint32_t millis) {
    struct timespec deadline;
    clock_gettime(CLOCK_MONOTONIC, &deadline);
    deadline.tv_sec += millis / 1000u;
    deadline.tv_nsec += (long)(millis % 1000u) * 1000000L;
    if (deadline.tv_nsec >= 1000000000L) {
        deadline.tv_sec++;
        deadline.tv_nsec -= 1000000000L;
    }
    return deadline;
}

static bool earlier(struct timespec a, struct timespec b) {
    return a.tv_sec < b.tv_sec || (a.tv_sec == b.tv_sec && a.tv_nsec < b.tv_nsec);
}

static int32_t fill(OH_AudioRenderer *renderer, void *user_data, void *audio_data, int32_t bytes) {
    (void)renderer;
    ShinkiAudioStream *stream = user_data;
    float *output = audio_data;
    pthread_mutex_lock(&stream->mutex);
    if (bytes < 0 || (size_t)bytes % (sizeof(float) * stream->channels)) {
        fail(stream, AUDIO_INVALID_PCM);
        pthread_mutex_unlock(&stream->mutex);
        return -1;
    }
    size_t wanted = (size_t)bytes / sizeof(float);
    size_t available = status(stream) ? 0 : stream->count < wanted ? stream->count : wanted;
    if (stream->clock) {
        if (available && !stream->clock->count) {
            stream->clock->deadline = after_ms(stream->stall_ms);
        }
        if (!record_span(stream->clock, wanted / stream->channels, available / stream->channels)) {
            fail(stream, AUDIO_CLOCK_OVERFLOW);
            pthread_mutex_unlock(&stream->mutex);
            memset(output, 0, (size_t)bytes);
            return -1;
        }
    }
    if (available) {
        size_t contiguous = stream->capacity - stream->read;
        if (contiguous > available) contiguous = available;
        memcpy(output, stream->samples + stream->read, contiguous * sizeof(float));
        memcpy(output + contiguous, stream->samples, (available - contiguous) * sizeof(float));
        stream->read = (stream->read + available) % stream->capacity;
        stream->count -= available;
        pthread_cond_broadcast(&stream->changed);
    }
    pthread_mutex_unlock(&stream->mutex);
    memset(output + available, 0, (wanted - available) * sizeof(float));
    return 0;
}

static void release(ShinkiAudioStream *stream) {
    pthread_cond_destroy(&stream->changed);
    pthread_mutex_destroy(&stream->mutex);
    free(stream->samples);
    free(stream->clock);
    free(stream);
}

// Opens and starts a 32-bit float renderer. Returns 0 or an OHAudio error.
int shinki_ohos_audio_open(ShinkiAudioStream **out, uint32_t sample_rate,
                            uint32_t channels, uint32_t stall_ms) {
    ShinkiAudioStream *stream = calloc(1, sizeof(*stream));
    if (!stream) return AUDIO_INTERNAL;
    stream->channels = channels;
    stream->stall_ms = stall_ms;
    if (pthread_mutex_init(&stream->mutex, NULL)) {
        free(stream);
        return AUDIO_INTERNAL;
    }
    pthread_condattr_t attr;
    int result_init = pthread_condattr_init(&attr);
    if (!result_init) {
        result_init = pthread_condattr_setclock(&attr, CLOCK_MONOTONIC);
        if (!result_init) result_init = pthread_cond_init(&stream->changed, &attr);
        pthread_condattr_destroy(&attr);
    }
    if (result_init) {
        pthread_mutex_destroy(&stream->mutex);
        free(stream);
        return AUDIO_INTERNAL;
    }
    stream->capacity = (size_t)QUEUE_FRAMES * channels;
    stream->samples = malloc(stream->capacity * sizeof(float));
    if (stall_ms) stream->clock = calloc(1, sizeof(*stream->clock));
    if (!stream->samples || (stall_ms && !stream->clock)) {
        release(stream);
        return AUDIO_INTERNAL;
    }
    OH_AudioStreamBuilder *builder = NULL;
    OH_AudioStream_Result result = OH_AudioStreamBuilder_Create(&builder, AUDIOSTREAM_TYPE_RENDERER);
    if (result == AUDIOSTREAM_SUCCESS) result = OH_AudioStreamBuilder_SetSamplingRate(builder, (int32_t)sample_rate);
    if (result == AUDIOSTREAM_SUCCESS) result = OH_AudioStreamBuilder_SetChannelCount(builder, (int32_t)channels);
    if (result == AUDIOSTREAM_SUCCESS) result = OH_AudioStreamBuilder_SetSampleFormat(builder, AUDIOSTREAM_SAMPLE_F32LE);
    if (result == AUDIOSTREAM_SUCCESS) result = OH_AudioStreamBuilder_SetRendererInfo(builder, AUDIOSTREAM_USAGE_GAME);
    // Register both callbacks together. Separate error registration needs
    // API 20; a separate write registration can overwrite the legacy set.
    OH_AudioRenderer_Callbacks callbacks = {
        .OH_AudioRenderer_OnWriteData = fill,
        .OH_AudioRenderer_OnError = on_error
    };
    if (result == AUDIOSTREAM_SUCCESS) result = OH_AudioStreamBuilder_SetRendererCallback(builder, callbacks, stream);
    if (result == AUDIOSTREAM_SUCCESS) result = OH_AudioStreamBuilder_GenerateRenderer(builder, &stream->renderer);
    if (builder) OH_AudioStreamBuilder_Destroy(builder);
    if (result == AUDIOSTREAM_SUCCESS) {
        result = OH_AudioRenderer_Start(stream->renderer);
        if (result != AUDIOSTREAM_SUCCESS) OH_AudioRenderer_Release(stream->renderer);
    }
    if (result != AUDIOSTREAM_SUCCESS) {
        release(stream);
        return (int)result;
    }
    *out = stream;
    return 0;
}

// Queues whole frames, waiting for space. Cancellation and native errors
// wake the same wait as the renderer callback.
int shinki_ohos_audio_write(ShinkiAudioStream *stream, const float *samples, size_t count) {
    if (count % stream->channels) return AUDIO_INVALID_PCM;
    refresh_clock(stream);
    pthread_mutex_lock(&stream->mutex);
    size_t written = 0;
    while (written < count && !status(stream)) {
        if (stream->count == stream->capacity) {
            struct timespec deadline = stream->stall_ms ? after_ms(stream->stall_ms) : (struct timespec){0};
            while (stream->count == stream->capacity && !status(stream)) {
                int waited = stream->stall_ms
                    ? pthread_cond_timedwait(&stream->changed, &stream->mutex, &deadline)
                    : pthread_cond_wait(&stream->changed, &stream->mutex);
                if (waited == ETIMEDOUT && stream->count == stream->capacity && !status(stream)) {
                    fail(stream, AUDIO_STALLED);
                } else if (waited && waited != ETIMEDOUT) {
                    fail(stream, AUDIO_INTERNAL);
                }
            }
            if (status(stream)) break;
        }
        size_t free_space = stream->capacity - stream->count;
        size_t chunk = count - written < free_space ? count - written : free_space;
        size_t tail = (stream->read + stream->count) % stream->capacity;
        size_t contiguous = stream->capacity - tail;
        if (contiguous > chunk) contiguous = chunk;
        memcpy(stream->samples + tail, samples + written, contiguous * sizeof(float));
        memcpy(stream->samples, samples + written + contiguous, (chunk - contiguous) * sizeof(float));
        stream->count += chunk;
        written += chunk;
        if (stream->clock) stream->clock->progress.accepted_frames += chunk / stream->channels;
    }
    int result = status(stream);
    pthread_mutex_unlock(&stream->mutex);
    return result;
}

void shinki_ohos_audio_progress(ShinkiAudioStream *stream, AudioProgress *progress) {
    refresh_clock(stream);
    pthread_mutex_lock(&stream->mutex);
    if (stream->clock) *progress = stream->clock->progress;
    pthread_mutex_unlock(&stream->mutex);
}

int shinki_ohos_audio_check(ShinkiAudioStream *stream) {
    pthread_mutex_lock(&stream->mutex);
    int result = stream->failure;
    pthread_mutex_unlock(&stream->mutex);
    return result;
}

int shinki_ohos_audio_drain(ShinkiAudioStream *stream) {
    PlaybackClock *clock = stream->clock;
    if (!clock) return AUDIO_INTERNAL;
    pthread_mutex_lock(&stream->mutex);
    uint64_t target = clock->progress.accepted_frames;
    uint64_t previous = clock->progress.played_frames;
    struct timespec deadline = after_ms(stream->stall_ms);
    while (clock->progress.played_frames < target && !status(stream)) {
        pthread_mutex_unlock(&stream->mutex);
        refresh_clock(stream);
        pthread_mutex_lock(&stream->mutex);
        if (clock->progress.played_frames >= target || status(stream)) break;
        if (clock->progress.played_frames != previous) {
            previous = clock->progress.played_frames;
            deadline = after_ms(stream->stall_ms);
        }
        if (!earlier(after_ms(0), deadline)) {
            fail(stream, AUDIO_STALLED);
            break;
        }
        struct timespec poll = after_ms(DRAIN_POLL_MS);
        if (earlier(deadline, poll)) poll = deadline;
        int waited = pthread_cond_timedwait(&stream->changed, &stream->mutex, &poll);
        if (waited && waited != ETIMEDOUT) fail(stream, AUDIO_INTERNAL);
    }
    int result = status(stream);
    pthread_mutex_unlock(&stream->mutex);
    return result;
}

// Wakes writers and tail drains without stopping the renderer under our mutex.
void shinki_ohos_audio_cancel(ShinkiAudioStream *stream) {
    pthread_mutex_lock(&stream->mutex);
    stream->cancelled = true;
    pthread_cond_broadcast(&stream->changed);
    pthread_mutex_unlock(&stream->mutex);
}

void shinki_ohos_audio_close(ShinkiAudioStream *stream) {
    OH_AudioRenderer_Stop(stream->renderer);
    OH_AudioRenderer_Release(stream->renderer);
    release(stream);
}
