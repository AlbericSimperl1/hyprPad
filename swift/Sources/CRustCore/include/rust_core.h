/*
 * rust_core.h — C-ABI tussen de Rust core en de Swift app (hyprPadClient).
 * Gegenereerd door cbindgen. Niet handmatig bewerken.
 */

#ifndef RUST_CORE_H
#define RUST_CORE_H

/* Waarschuwing: automatisch gegenereerd door cbindgen. */

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

#define NAL_NON_IDR 1

#define NAL_IDR 5

#define NAL_SEI 6

#define NAL_SPS 7

#define NAL_PPS 8

/**
 * NALU-callback. Wordt aangeroepen op de `nalu-parser` thread (background).
 * De pointer is enkel geldig tijdens de call — kopieer de bytes in Swift.
 */
typedef void (*OnNalu)(const uint8_t *data, uint32_t len, uint8_t nal_type, void *ctx);

/**
 * Log-callback. `level`: 0=info, 1=warn, 2=error. Swift moet altijd een geldige
 * functie meegeven (geen `Option`) — cbindgen vertaalt `Option<fn>` namelijk
 * niet naar een C-function-pointer.
 */
typedef void (*OnLog)(uint8_t level, const char *msg, void *ctx);

/**
 * Callbacks die Swift meegeeft. Eén struct = geen volgorderisico's.
 * Beide velden zijn verplicht (geen NULL).
 */
typedef struct {
    OnNalu on_nalu;
    OnLog on_log;
} HyprpadCallbacks;

/**
 * State van de engine, opgevraagd via `hyprpad_stats()`.
 * `state`: 0=idle, 1=listening (socket open), 2=decoding (frames ontvangen),
 * 3=error.
 */
typedef struct {
    uint32_t fps;
    uint64_t bytes_total;
    uint32_t width;
    uint32_t height;
    uint8_t state;
} HyprpadStats;

/**
 * Start de UDP-listener + NALU-parser op `port`.
 *
 * # Safety
 * - Niet twee keer aanroepen zonder `hyprpad_stop` ertussen.
 * - `ctx` wordt ongewijzigd teruggegeven aan elke callback.
 */
bool hyprpad_start(uint16_t port, HyprpadCallbacks callbacks, void *ctx);

/**
 * Stop de actieve stream en join de worker-threads.
 */
void hyprpad_stop(void);

/**
 * Poll de stats. Veilig om vanaf elke thread (incl. main) aangeroepen te worden.
 */
HyprpadStats hyprpad_stats(void);

#endif  /* RUST_CORE_H */
