/* CMVS 48FC10/48FE70 setup and 48FAA0 conversion, using the Windows x86 ACM ABI.
 * No codec is pinned by CMVS: NULL drivers preserve installed x86 priority.
 * The sink first receives an 88-byte CAC1 header, then initialized PCM chunks.
 * Return codes describe a PCM-use boundary; they never erase the queried DWORD.
 */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <mmsystem.h>
#include <mmreg.h>
#include <msacm.h>
#include <stdint.h>
#include <string.h>
#ifndef CMVS_ACM_LIBRARY
#include <stdio.h>
#include <stdlib.h>
#include <io.h>
#include <fcntl.h>
#endif

typedef int (__cdecl *CmvsSink)(void *, const unsigned char *, uint32_t);
typedef char CmvsNativeAcmWire[(sizeof(WAVEFORMATEX) == 18
    && sizeof(MPEGLAYER3WAVEFORMAT) == 30 && sizeof(ACMSTREAMHEADER) == 84) ? 1 : -1];
static void put32(unsigned char *p, uint32_t n) { memcpy(p, &n, 4); }

/* Codes: 0 initialized PCM, 1 unsupported suggested PCM, 2 prepare,
 * 3 convert, 4 unprepare, 5 invalid used length, 6 byte bound, 7 sink failure,
 * 8 no initialized PCM, 9 inaccessible source extent, 10 invalid MP3 wire.
 * Stream closure is unconditional on every exit.
 */
int __cdecl cmvs_acm_decode(const unsigned char *format, const unsigned char *source,
    uint32_t length, uint32_t available, uint32_t max_pcm, CmvsSink sink, void *user)
{
    MPEGLAYER3WAVEFORMAT src;
    WAVEFORMATEX dst;
    HACMSTREAM stream = NULL;
    HACMDRIVERID driver = NULL;
    ACMDRIVERDETAILSA details;
    unsigned char wire[88];
    unsigned char pcm[0x8000];
    DWORD extent = 0, position = 0, produced = 0;
    MMRESULT suggest_status, open_status, size_status, details_status;
    int status = 0;
    memcpy(&src, format, 30);
    if (src.wfx.wFormatTag != 0x55 || src.wfx.cbSize != 12) return 10;
    memset(&dst, 0, 18);
    dst.wFormatTag = WAVE_FORMAT_PCM;
    suggest_status = acmFormatSuggest(NULL, (LPWAVEFORMATEX)&src, &dst, 18, 0x10000);
    open_status = acmStreamOpen(&stream, NULL, (LPWAVEFORMATEX)&src, &dst, NULL, 0, 0, 0);
    /* Even failed suggestion/open reaches StreamSize in the original. With
     * a valid output pointer its documented error result defines zero. */
    size_status = acmStreamSize(stream, length, &extent, 0);
    memset(&details, 0, sizeof(details));
    details.cbStruct = sizeof(details);
    details_status = acmDriverID((HACMOBJ)stream, &driver, 0);
    if (details_status == 0) details_status = acmDriverDetailsA(driver, &details, 0);
    memset(wire, 0, sizeof(wire));
    memcpy(wire, "CAC1", 4);
    put32(wire + 4, extent);
    memcpy(wire + 8, &dst, 18);
    put32(wire + 28, suggest_status);
    put32(wire + 32, open_status);
    put32(wire + 36, size_status);
    put32(wire + 40, details_status);
    put32(wire + 44, details.vdwDriver);
    put32(wire + 48, details.wMid);
    put32(wire + 52, details.wPid);
    memcpy(wire + 56, details.szShortName, 32);
    if (!sink(user, wire, sizeof(wire))) { status = 7; goto done; }
    if (dst.wFormatTag != 1 || (dst.nChannels != 1 && dst.nChannels != 2)
        || dst.wBitsPerSample != 16 || dst.nSamplesPerSec == 0
        || dst.nBlockAlign != 2 * dst.nChannels) { status = 1; goto done; }
    if (length > available) { status = 9; goto done; }
    while (position < length) {
        ACMSTREAMHEADER header;
        MMRESULT prepared, converted, unprepared;
        memset(&header, 0, sizeof(header));
        header.cbStruct = sizeof(header);
        header.pbSrc = (LPBYTE)source + position;
        header.cbSrcLength = length - position;
        header.pbDst = pcm;
        header.cbDstLength = sizeof(pcm);
        prepared = acmStreamPrepareHeader(stream, &header, 0);
        converted = acmStreamConvert(stream, &header, 0);
        unprepared = acmStreamUnprepareHeader(stream, &header, 0);
        if (unprepared != 0 && prepared == 0) {
            /* Keep the prepared header alive until cleanup releases it. */
            acmStreamReset(stream, 0);
            acmStreamUnprepareHeader(stream, &header, 0);
            status = 4;
            goto done;
        }
        /* Native consumes cb*Used despite errors. The API does not promise
         * initialized PCM on failure: retain its size DWORD and expose that
         * PCM-use boundary instead of reading uninitialized scratch. */
        if (prepared != 0) { status = 2; goto done; }
        if (converted != 0) { status = 3; goto done; }
        /* Native EOF is zero output, with no additional END flush. */
        if (header.cbDstLengthUsed == 0) {
            if (produced == 0) status = 8;
            break;
        }
        if (header.cbSrcLengthUsed > length - position
            || header.cbDstLengthUsed > sizeof(pcm)) { status = 5; goto done; }
        if (header.cbDstLengthUsed > max_pcm - produced) { status = 6; goto done; }
        if (!sink(user, pcm, header.cbDstLengthUsed)) { status = 7; goto done; }
        produced += header.cbDstLengthUsed;
        position += header.cbSrcLengthUsed;
    }
    if (produced == 0 && status == 0) status = 8;
done:
    if (stream != NULL) acmStreamClose(stream, 0);
    return status;
}

#ifndef CMVS_ACM_LIBRARY
struct Output { int header; };
static int __cdecl output(void *opaque, const unsigned char *bytes, uint32_t length)
{
    struct Output *state = (struct Output *)opaque;
    if (state->header) {
        if (fwrite(&length, 4, 1, stdout) != 1) return 0;
    } else { state->header = 1; }
    return fwrite(bytes, 1, length, stdout) == length;
}
int main(void)
{
    unsigned char format[30];
    unsigned char *source;
    uint32_t length, available, max_pcm, zero = 0, status;
    struct Output out = { 0 };
    _setmode(_fileno(stdin), _O_BINARY);
    _setmode(_fileno(stdout), _O_BINARY);
    if (fread(format, 1, 30, stdin) != 30 || fread(&length, 4, 1, stdin) != 1
        || fread(&available, 4, 1, stdin) != 1 || fread(&max_pcm, 4, 1, stdin) != 1
        || available > 64u * 1024u * 1024u || max_pcm > 256u * 1024u * 1024u) return 10;
    source = (unsigned char *)malloc(available ? available : 1);
    if (source == NULL) return 11;
    if (fread(source, 1, available, stdin) != available) { free(source); return 12; }
    status = (uint32_t)cmvs_acm_decode(format, source, length, available, max_pcm, output, &out);
    free(source);
    if (!out.header || fwrite(&zero, 4, 1, stdout) != 1 || fwrite(&status, 4, 1, stdout) != 1
        || fflush(stdout) != 0) return 13;
    return 0;
}
#endif
