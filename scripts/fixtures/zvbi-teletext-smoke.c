#include <inttypes.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <windows.h>

#include <libavcodec/avcodec.h>
#include <libavutil/opt.h>
#include <libzvbi.h>

static void require(int condition, const char *message)
{
    if (!condition) {
        fprintf(stderr, "%s\n", message);
        exit(1);
    }
}

static uint8_t reverse_bits(uint8_t value)
{
    uint8_t result = 0;
    for (unsigned int bit = 0; bit < 8; ++bit) {
        result = (uint8_t)((result << 1) | (value & 1));
        value >>= 1;
    }
    return result;
}

static uint8_t odd_parity(uint8_t value)
{
    unsigned int parity = 1;
    for (unsigned int bit = 0; bit < 7; ++bit)
        parity ^= (value >> bit) & 1;
    return (uint8_t)((value & 0x7f) | (parity << 7));
}

static void row(uint8_t *unit, unsigned int magazine, unsigned int number,
                unsigned int variant)
{
    static const uint8_t hamming[16] = {
        0x15, 0x02, 0x49, 0x5e, 0x64, 0x73, 0x38, 0x2f,
        0xd0, 0xc7, 0x8c, 0x9b, 0xa1, 0xb6, 0xfd, 0xea
    };
    static const char *text[] = {
        "\013\013TOWAVUE Teletext 0123456789\012\012",
        "\013\013\002Green\007 White\015 Double height\012\012",
        "\013\013National characters #$@[\\]^_`{|}~\012\012",
        "\013\013\021\077\077\032\077\077\007 Mosaic and text\012\012"
    };
    uint8_t data[42];
    memset(data, odd_parity(' '), sizeof(data));
    data[0] = hamming[(magazine & 7) | ((number & 1) << 3)];
    data[1] = hamming[number >> 1];
    if (number == 0) {
        for (unsigned int i = 2; i < 10; ++i)
            data[i] = hamming[0];
        data[2] = hamming[(variant >> 4) & 15];
        data[5] = hamming[8]; /* Erase page. */
        data[7] = hamming[8]; /* Subtitle. */
        data[8] = hamming[1]; /* Suppress header. */
        data[9] = hamming[1 | ((variant & 1) << 1)]; /* Serial, national subset. */
    } else if (number == 30) {
        /* Broadcast service data must not enable program-ID/time events. */
        memset(data + 2, hamming[0], 40);
        data[2] = hamming[variant & 2];
    } else {
        const char *line = text[variant % 4];
        for (size_t i = 0; line[i] && i < 40; ++i)
            data[i + 2] = odd_parity((uint8_t)line[i]);
    }
    unit[0] = 3;
    unit[1] = 44;
    unit[2] = 0x27;
    unit[3] = 0xe4;
    for (unsigned int i = 0; i < 42; ++i)
        unit[i + 4] = reverse_bits(data[i]);
}

static unsigned int record(FILE *output, AVSubtitle *subtitle, unsigned int variant)
{
    unsigned int nonempty = 0;
    fprintf(output, "subtitle %u %u %u %u %" PRId64 "\n", subtitle->format,
            subtitle->start_display_time, subtitle->end_display_time,
            subtitle->num_rects, subtitle->pts);
    for (unsigned int i = 0; i < subtitle->num_rects; ++i) {
        AVSubtitleRect *rect = subtitle->rects[i];
        fprintf(output, "rect %d %d %d %d %d %d %d\n", rect->type, rect->x,
                rect->y, rect->w, rect->h, rect->nb_colors, rect->flags);
        if (rect->type == SUBTITLE_BITMAP) {
            require(rect->w > 0 && rect->h > 0 && rect->nb_colors > 0,
                    "Empty bitmap subtitle.");
            for (int y = 0; y < rect->h; ++y)
                require(fwrite(rect->data[0] + y * rect->linesize[0], 1,
                               (size_t)rect->w, output) == (size_t)rect->w,
                        "Cannot write bitmap evidence.");
            require(fwrite(rect->data[1], 4, (size_t)rect->nb_colors, output)
                    == (size_t)rect->nb_colors, "Cannot write palette evidence.");
            ++nonempty;
        } else if (rect->ass && rect->ass[0]) {
            if (variant == 0)
                require(strstr(rect->ass, "TOWAVUE Teletext 0123456789") != NULL ||
                        strstr(rect->ass, "TOWAVUE\\hTeletext\\h0123456789") != NULL,
                        "Expected subtitle text was not decoded.");
            fprintf(output, "ass %zu:%s\n", strlen(rect->ass), rect->ass);
            ++nonempty;
        } else if (rect->text && rect->text[0]) {
            if (variant == 0)
                require(strstr(rect->text, "TOWAVUE Teletext 0123456789") != NULL,
                        "Expected subtitle text was not decoded.");
            fprintf(output, "text %zu:%s\n", strlen(rect->text), rect->text);
            ++nonempty;
        }
    }
    avsubtitle_free(subtitle);
    return nonempty;
}

static void event_handler(vbi_event *event, void *data)
{
    (void)event;
    (void)data;
}

int main(int argc, char **argv)
{
    require(argc == 4, "Usage: zvbi-teletext-smoke output.bin expected-dll scope(0|1)");
    require(!strcmp(argv[3], "0") || !strcmp(argv[3], "1"), "Invalid scope.");
    char module[MAX_PATH];
    DWORD length = GetModuleFileNameA(GetModuleHandleA("libzvbi-0.dll"), module, MAX_PATH);
    require(length > 0 && length < MAX_PATH, "Cannot identify loaded ZVBI.");
    for (DWORD i = 0; i < length; ++i)
        if (module[i] == '\\') module[i] = '/';
    require(!_stricmp(module, argv[2]), "Loaded a different ZVBI DLL.");
    printf("Loaded ZVBI: %s\n", module);
    vbi_decoder *decoder = vbi_decoder_new();
    require(decoder != NULL, "Cannot create ZVBI decoder.");
    require(vbi_event_handler_register(decoder, VBI_EVENT_TTX_PAGE, event_handler, NULL),
            "Cannot register teletext events.");
    int allowed = argv[3][0] == '1';
    require(!!vbi_event_handler_register(decoder, VBI_EVENT_PROG_ID, event_handler, NULL)
            == allowed, "Unexpected program-ID registration result.");
    require(!!vbi_event_handler_add(decoder, VBI_EVENT_LOCAL_TIME, event_handler, NULL)
            == allowed, "Unexpected local-time registration result.");
    vbi_decoder_delete(decoder);

    FILE *output = fopen(argv[1], "wb");
    require(output != NULL, "Cannot create comparison evidence.");
    fprintf(output, "abi %zu %zu %zu %zu\n", sizeof(vbi_event),
            offsetof(vbi_event, ev.ttx_page), sizeof(vbi_page), sizeof(vbi_char));
    const AVCodec *codec = avcodec_find_decoder_by_name("libzvbi_teletextdec");
    require(codec != NULL, "FFmpeg teletext decoder is absent.");
    for (unsigned int format = 0; format < 3; ++format) {
        for (unsigned int variant = 0; variant < 4; ++variant) {
            AVCodecContext *context = avcodec_alloc_context3(codec);
            require(context != NULL, "Cannot allocate subtitle context.");
            require(av_opt_set_int(context->priv_data, "txt_format", format, 0) == 0,
                    "Cannot select subtitle format.");
            require(av_opt_set(context->priv_data, "txt_page", "subtitle", 0) == 0,
                    "Cannot select subtitle pages.");
            context->pkt_timebase = (AVRational){1, 90000};
            require(avcodec_open2(context, codec, NULL) == 0, "Cannot open subtitle decoder.");
            AVPacket *packet = av_packet_alloc();
            require(packet && av_new_packet(packet, 139) == 0, "Cannot allocate subtitle packet.");
            unsigned int nonempty = 0;
            fprintf(output, "case %u %u\n", format, variant);
            for (unsigned int frame = 0; frame < 4; ++frame) {
                packet->data[0] = 0x10;
                row(packet->data + 1, 1, 0, variant | ((frame & 1) << 4));
                row(packet->data + 47, 1, 1, variant);
                row(packet->data + 93, frame & 1 ? 0 : 1,
                    frame & 1 ? 30 : 2, variant);
                packet->pts = frame * 90000;
                AVSubtitle subtitle = {0};
                int got = 0;
                require(avcodec_decode_subtitle2(context, &subtitle, &got, packet) >= 0,
                        "FFmpeg rejected valid synthetic subtitle packet.");
                if (got) nonempty += record(output, &subtitle, variant);
            }
            require(nonempty == 3, "Expected three nonempty subtitle updates for this case.");
            packet->data[2] = 43;
            AVSubtitle invalid = {0};
            int got = 0;
            require(avcodec_decode_subtitle2(context, &invalid, &got, packet) < 0,
                    "Malformed teletext data-unit length was accepted.");
            avsubtitle_free(&invalid);
            av_packet_free(&packet);
            avcodec_free_context(&context);
        }
    }
    require(!ferror(output) && fclose(output) == 0, "Cannot finish comparison evidence.");
    puts("Twelve nonempty FFmpeg teletext cases and malformed-packet rejection passed.");
    return 0;
}
