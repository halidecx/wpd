/*
 * Copyright (c) 2009 Mans Rullgard <mans@mansr.com>
 *
 * This file is part of FFmpeg.
 *
 * FFmpeg is free software; you can redistribute it and/or
 * modify it under the terms of the GNU Lesser General Public
 * License as published by the Free Software Foundation; either
 * version 2.1 of the License, or (at your option) any later version.
 *
 * FFmpeg is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
 * Lesser General Public License for more details.
 *
 * You should have received a copy of the GNU Lesser General Public
 * License along with FFmpeg; if not, write to the Free Software
 * Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301 USA
 */

#include "asm.S"

        .macro ldcol.8  rd,  rs,  rt,  n=8,  hi=0
.if \n == 8 || \hi == 0
        vld1.8          {\rd[0]}, [\rs], \rt
        vld1.8          {\rd[1]}, [\rs], \rt
        vld1.8          {\rd[2]}, [\rs], \rt
        vld1.8          {\rd[3]}, [\rs], \rt
.endif
.if \n == 8 || \hi == 1
        vld1.8          {\rd[4]}, [\rs], \rt
        vld1.8          {\rd[5]}, [\rs], \rt
        vld1.8          {\rd[6]}, [\rs], \rt
        vld1.8          {\rd[7]}, [\rs], \rt
.endif
        .endm

        .macro add16x8  dq,  dl,  dh,  rl,  rh
        vaddl.u8        \dq, \rl, \rh
        vadd.u16        \dl, \dl, \dh
        vpadd.u16       \dl, \dl, \dl
        vpadd.u16       \dl, \dl, \dl
        .endm

function ff_pred16x16_128_dc_neon, export=1
        vmov.i8         q0,  #128
        b               .L_pred16x16_dc_end
endfunc

function ff_pred16x16_top_dc_neon, export=1
        sub             r2,  r0,  r1
        vld1.8          {q0},     [r2,:128]
        add16x8         q0,  d0,  d1,  d0,  d1
        vrshrn.u16      d0,  q0,  #4
        vdup.8          q0,  d0[0]
        b               .L_pred16x16_dc_end
endfunc

function ff_pred16x16_left_dc_neon, export=1
        sub             r2,  r0,  #1
        ldcol.8         d0,  r2,  r1
        ldcol.8         d1,  r2,  r1
        add16x8         q0,  d0,  d1,  d0,  d1
        vrshrn.u16      d0,  q0,  #4
        vdup.8          q0,  d0[0]
        b               .L_pred16x16_dc_end
endfunc

function ff_pred16x16_dc_neon, export=1
        sub             r2,  r0,  r1
        vld1.8          {q0},     [r2,:128]
        sub             r2,  r0,  #1
        ldcol.8         d2,  r2,  r1
        ldcol.8         d3,  r2,  r1
        vaddl.u8        q0,  d0,  d1
        vaddl.u8        q1,  d2,  d3
        vadd.u16        q0,  q0,  q1
        vadd.u16        d0,  d0,  d1
        vpadd.u16       d0,  d0,  d0
        vpadd.u16       d0,  d0,  d0
        vrshrn.u16      d0,  q0,  #5
        vdup.8          q0,  d0[0]
.L_pred16x16_dc_end:
        mov             r3,  #8
6:      vst1.8          {q0},     [r0,:128], r1
        vst1.8          {q0},     [r0,:128], r1
        subs            r3,  r3,  #1
        bne             6b
        bx              lr
endfunc

function ff_pred16x16_hor_neon, export=1
        sub             r2,  r0,  #1
        mov             r3,  #16
1:      vld1.8          {d0[],d1[]},[r2],      r1
        vst1.8          {q0},       [r0,:128], r1
        subs            r3,  r3,  #1
        bne             1b
        bx              lr
endfunc

function ff_pred16x16_vert_neon, export=1
        sub             r0,  r0,  r1
        vld1.8          {q0},     [r0,:128], r1
        mov             r3,  #8
1:      vst1.8          {q0},     [r0,:128], r1
        vst1.8          {q0},     [r0,:128], r1
        subs            r3,  r3,  #1
        bne             1b
        bx              lr
endfunc
function ff_pred8x8_hor_neon, export=1
        sub             r2,  r0,  #1
        mov             r3,  #8
1:      vld1.8          {d0[]},   [r2],     r1
        vst1.8          {d0},     [r0,:64], r1
        subs            r3,  r3,  #1
        bne             1b
        bx              lr
endfunc

function ff_pred8x8_vert_neon, export=1
        sub             r0,  r0,  r1
        vld1.8          {d0},     [r0,:64], r1
        mov             r3,  #4
1:      vst1.8          {d0},     [r0,:64], r1
        vst1.8          {d0},     [r0,:64], r1
        subs            r3,  r3,  #1
        bne             1b
        bx              lr
endfunc

function ff_pred8x8_128_dc_neon, export=1
        vmov.i8         d0,  #128
        mov             r3,  #8
1:      vst1.8          {d0}, [r0,:64], r1
        subs            r3,  r3,  #1
        bne             1b
        bx              lr
endfunc
