// OHAudio renderer behind the SDK AudioSink used by statically embedded
// families. Writers block on a bounded interleaved f32 queue; the renderer
// callback drains it and pads underruns with silence.
#include <ohaudio/native_audiorenderer.h>
#include <ohaudio/native_audiostreambuilder.h>
#include <pthread.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

// About 100 ms at 48 kHz; bounds latency while absorbing scheduler jitter.
#define QUEUE_FRAMES 4800u

typedef struct {
    pthread_mutex_t mutex;
    pthread_cond_t space;
    OH_AudioRenderer *renderer;
    float *samples;
    size_t capacity;
    size_t read;
    size_t count;
    bool cancelled;
} ShinkiAudioStream;

static OH_AudioData_Callback_Result fill(OH_AudioRenderer *renderer, void *user_data, void *audio_data, int32_t bytes) {
    (void)renderer;
    ShinkiAudioStream *stream = user_data;
    float *output = audio_data;
    size_t wanted = (size_t)bytes / sizeof(float);
    pthread_mutex_lock(&stream->mutex);
    size_t available = stream->count < wanted ? stream->count : wanted;
    for (size_t index = 0; index < available; index++) {
        output[index] = stream->samples[(stream->read + index) % stream->capacity];
    }
    stream->read = (stream->read + available) % stream->capacity;
    stream->count -= available;
    pthread_cond_broadcast(&stream->space);
    pthread_mutex_unlock(&stream->mutex);
    memset(output + available, 0, (wanted - available) * sizeof(float));
    return AUDIO_DATA_CALLBACK_RESULT_VALID;
}

static void release(ShinkiAudioStream *stream) {
    pthread_cond_destroy(&stream->space);
    pthread_mutex_destroy(&stream->mutex);
    free(stream->samples);
    free(stream);
}

// Opens and starts a 32-bit float renderer. Returns 0 or an OHAudio error.
int shinki_ohos_audio_open(ShinkiAudioStream **out, uint32_t sample_rate, uint32_t channels) {
    ShinkiAudioStream *stream = calloc(1, sizeof(*stream));
    if (!stream) return -1;
    stream->capacity = (size_t)QUEUE_FRAMES * channels;
    stream->samples = calloc(stream->capacity, sizeof(float));
    pthread_mutex_init(&stream->mutex, NULL);
    pthread_cond_init(&stream->space, NULL);
    if (!stream->samples) {
        release(stream);
        return -1;
    }
    OH_AudioStreamBuilder *builder = NULL;
    OH_AudioStream_Result result = OH_AudioStreamBuilder_Create(&builder, AUDIOSTREAM_TYPE_RENDERER);
    if (result == AUDIOSTREAM_SUCCESS) result = OH_AudioStreamBuilder_SetSamplingRate(builder, (int32_t)sample_rate);
    if (result == AUDIOSTREAM_SUCCESS) result = OH_AudioStreamBuilder_SetChannelCount(builder, (int32_t)channels);
    if (result == AUDIOSTREAM_SUCCESS) result = OH_AudioStreamBuilder_SetSampleFormat(builder, AUDIOSTREAM_SAMPLE_F32LE);
    if (result == AUDIOSTREAM_SUCCESS) result = OH_AudioStreamBuilder_SetRendererInfo(builder, AUDIOSTREAM_USAGE_GAME);
    if (result == AUDIOSTREAM_SUCCESS) result = OH_AudioStreamBuilder_SetRendererWriteDataCallback(builder, fill, stream);
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

// Queues `count` interleaved samples, waiting for space. Returns 0 when all
// were queued and 1 when the stream was cancelled first.
int shinki_ohos_audio_write(ShinkiAudioStream *stream, const float *samples, size_t count) {
    pthread_mutex_lock(&stream->mutex);
    size_t written = 0;
    while (written < count && !stream->cancelled) {
        while (stream->count == stream->capacity && !stream->cancelled) {
            pthread_cond_wait(&stream->space, &stream->mutex);
        }
        size_t free_space = stream->capacity - stream->count;
        size_t chunk = count - written < free_space ? count - written : free_space;
        size_t tail = (stream->read + stream->count) % stream->capacity;
        for (size_t index = 0; index < chunk; index++) {
            stream->samples[(tail + index) % stream->capacity] = samples[written + index];
        }
        stream->count += chunk;
        written += chunk;
    }
    bool cancelled = stream->cancelled;
    pthread_mutex_unlock(&stream->mutex);
    return cancelled ? 1 : 0;
}

// Wakes a blocked writer; later writes return 1 immediately.
void shinki_ohos_audio_cancel(ShinkiAudioStream *stream) {
    pthread_mutex_lock(&stream->mutex);
    stream->cancelled = true;
    pthread_cond_broadcast(&stream->space);
    pthread_mutex_unlock(&stream->mutex);
}

void shinki_ohos_audio_close(ShinkiAudioStream *stream) {
    OH_AudioRenderer_Stop(stream->renderer);
    OH_AudioRenderer_Release(stream->renderer);
    release(stream);
}
