<!-- SPDX-License-Identifier: MIT -->
# Third-party notices

famicom-rust is MIT licensed (LICENSE). It is a transcription of famicom-python (MIT), whose
`NOTICE.md` this file carries down for what the transcription holds; it contains no code from
the GPL emulators the reference's documents cite.

## MAME (BSD-3-Clause)

The reference took a few tables and values from MAME's BSD-3-Clause files (the first line of
each, `// license:BSD-3-Clause`, checked on 2026-11-23 in the reference's pinned copies, MAME
`e31391f` and `f379cd0`), and this transcription holds them as it does. The files that hold one
say `MIT AND BSD-3-Clause` on their SPDX line, with the source on the next:

- `src/cartridge/namco.rs`: the Vs. System's protection bytes (`vs_sequence`: the 32-byte
  answers of the chips on *Vs. Atari R.B.I. Baseball* and *Vs. T.K.O. Boxing*; `vs_xevious`:
  *Vs. Super Xevious*'s four addresses), from `src/mame/nintendo/vsnes.cpp`. Copyright
  Pierpaolo Prazzoli, Brad Oliver, Fabio Priuli.
- `src/machine.rs`: the RC2C05-01's `$2002` identifier, `$1B`, MAME's choice in
  `src/devices/video/ppu2c0x.cpp`. Copyright Ernesto Corvi, Brad Oliver, Fabio Priuli.
- `src/supervisor.rs`: the PlayChoice-10 BIOS files' names and SHA-1s and the screen's resistor
  weights (`0x0E`, `0x1F`, `0x43`, `0x8F`), from `src/mame/nintendo/playch10.cpp` (copyright
  Ernesto Corvi, Brad Oliver); the RP5H01's behaviour as `src/devices/machine/rp5h01.cpp`
  models it (copyright Nicola Salmoria).
- `src/peripherals.rs`: the Subor Keyboard's matrix, `SUBOR_MATRIX`, from
  `src/devices/bus/nes_ctrl/suborkey.cpp`. Copyright Fabio Priuli.
- `src/cartridge/waixing.rs`: the GAL table of mappers 252/253 (`waixing_gal`), from
  `src/devices/bus/nes/vrc_clones.cpp`. Copyright kmg.

Facts the reference read in MAME's BSD-3 files (the Vs. System's and the PlayChoice-10's
wiring, the Turbo File, Exciting Boxing's bag, several mappers) are cited where they are used;
those files stay `MIT`. The reference's `NOTICE.md` lists them.

The licence of those files:

```
Copyright (c) the copyright holders named above (MAME, https://www.mamedev.org)

Redistribution and use in source and binary forms, with or without modification, are permitted
provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this list of
   conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright notice, this list of
   conditions and the following disclaimer in the documentation and/or other materials provided
   with the distribution.
3. Neither the name of the copyright holder nor the names of its contributors may be used to
   endorse or promote products derived from this software without specific prior written
   permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR
IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND
FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR
CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER
IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT
OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

