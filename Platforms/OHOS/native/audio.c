#include <ohaudio/native_audiostreambuilder.h>
#include <pthread.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    pthread_mutex_t mutex;
    float *samples;
    uint32_t capacity;
    uint32_t read;
    uint32_t write;
} ShinkiOHAudioQueue;

static int32_t audio_write(OH_AudioRenderer *renderer, void *user_data, void *audio_data, int32_t bytes) {
    (void)renderer;
    ShinkiOHAudioQueue *queue = user_data;
    float *output = audio_data;
    uint32_t frames = (uint32_t)bytes / (sizeof(float) * 2);
    pthread_mutex_lock(&queue->mutex);
    uint32_t available = queue->write - queue->read;
    uint32_t count = available < frames ? available : frames;
    for (uint32_t index = 0; index < count; index++) {
        uint32_t offset = (queue->read + index) % queue->capacity;
        output[index * 2] = queue->samples[offset * 2];
        output[index * 2 + 1] = queue->samples[offset * 2 + 1];
    }
    queue->read += count;
    pthread_mutex_unlock(&queue->mutex);
    memset(output + count * 2, 0, (frames - count) * sizeof(float) * 2);
    return 0;
}

int shinki_ohos_audio_open(ShinkiOHAudioQueue **queue_out, OH_AudioRenderer **renderer_out) {
    ShinkiOHAudioQueue *queue = calloc(1, sizeof(*queue));
    if (!queue) return -1;
    queue->capacity = 48000 * 2;
    queue->samples = calloc(queue->capacity * 2, sizeof(float));
    pthread_mutex_init(&queue->mutex, NULL);
    OH_AudioStreamBuilder *builder = NULL;
    int result = OH_AudioStreamBuilder_Create(&builder, AUDIOSTREAM_TYPE_RENDERER);
    OH_AudioRenderer_Callbacks callbacks = { .OH_AudioRenderer_OnWriteData = audio_write };
    if (result == AUDIOSTREAM_SUCCESS) result = OH_AudioStreamBuilder_SetSamplingRate(builder, 48000);
    if (result == AUDIOSTREAM_SUCCESS) result = OH_AudioStreamBuilder_SetChannelCount(builder, 2);
    if (result == AUDIOSTREAM_SUCCESS) result = OH_AudioStreamBuilder_SetSampleFormat(builder, AUDIOSTREAM_SAMPLE_F32LE);
    if (result == AUDIOSTREAM_SUCCESS) result = OH_AudioStreamBuilder_SetRendererCallback(builder, callbacks, queue);
    if (result == AUDIOSTREAM_SUCCESS) result = OH_AudioStreamBuilder_GenerateRenderer(builder, renderer_out);
    OH_AudioStreamBuilder_Destroy(builder);
    if (result != AUDIOSTREAM_SUCCESS) { free(queue->samples); free(queue); return result; }
    result = OH_AudioRenderer_Start(*renderer_out);
    if (result != AUDIOSTREAM_SUCCESS) { OH_AudioRenderer_Release(*renderer_out); free(queue->samples); free(queue); return result; }
    *queue_out = queue;
    return 0;
}

int shinki_ohos_audio_write(ShinkiOHAudioQueue *queue, const float *samples, uint32_t frames) {
    pthread_mutex_lock(&queue->mutex);
    uint32_t used = queue->write - queue->read;
    if (frames > queue->capacity - used) { pthread_mutex_unlock(&queue->mutex); return -1; }
    for (uint32_t index = 0; index < frames; index++) {
        uint32_t offset = (queue->write + index) % queue->capacity;
        queue->samples[offset * 2] = samples[index * 2];
        queue->samples[offset * 2 + 1] = samples[index * 2 + 1];
    }
    queue->write += frames;
    pthread_mutex_unlock(&queue->mutex);
    return 0;
}

void shinki_ohos_audio_close(ShinkiOHAudioQueue *queue, OH_AudioRenderer *renderer) {
    OH_AudioRenderer_Stop(renderer);
    OH_AudioRenderer_Release(renderer);
    pthread_mutex_destroy(&queue->mutex);
    free(queue->samples);
    free(queue);
}
