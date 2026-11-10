; Rendering turned off in the middle of sprite evaluation, at a different
; dot each frame, and what evaluation left behind shown afterwards.
;
; Each frame: in vertical blank, read $2004 (OAM at the address evaluation
; left in OAMADDR when rendering went off: nothing resets it while rendering
; stays off), then refill OAM by DMA (OAM[i] = i, so the address shows in the
; value; sprite Ys 0-252 put sprites in range on every line), turn rendering
; on, wait a delay that grows by 5 CPU cycles a frame (and the $2002 poll's
; jitter), and write $2001 = 0: somewhere in lines 1-12, in or out of dots
; 65-256. Re-enabling the next frame also applies the PPU's OAM corruption
; from the turn-off.
;
; Written for famicom-rust's lockstep gate (the reference, famicom-python,
; is the judge; there is no pass or fail on screen). ca65: probes/build.sh.

.segment "HEADER"
    .byte "NES", $1A, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0

.segment "ZEROPAGE"
frame:  .res 1
seen:   .res 1

.segment "CODE"
reset:
    sei
    cld
    ldx #$FF
    txs
    lda #0
    sta $2000
    sta $2001
    sta frame
    bit $2002
@wait1:
    bit $2002
    bpl @wait1
@wait2:
    bit $2002
    bpl @wait2
    ldx #0
@fill:
    txa
    sta $0200,x
    inx
    bne @fill

main:
@vblank:
    bit $2002
    bpl @vblank
    lda $2004               ; OAM[OAMADDR] as the turn-off left it
    sta seen
    lda $2004
    lda #$02
    sta $4014               ; OAM[i] = i again (and the corruption undone)
    lda #0
    sta $2000
    sta $2005
    sta $2005
    lda #$18
    sta $2001               ; sprites and background on
    ldy #2                  ; about 2,000 cycles: into line 1
@base:
    ldx #200
@inner:
    dex
    bne @inner
    dey
    bne @base
    ldx frame               ; and 5 more cycles each frame
    inx
@sweep:
    dex
    bne @sweep
    lda #0
    sta $2001               ; off, mid-line
    inc frame
    jmp main

nmi:
irq:
    rti

.segment "VECTORS"
    .word nmi, reset, irq
