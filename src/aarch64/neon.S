/*
 * This file is part of FFmpeg.
 *
 * Copyright (c) 2023 J. Dekker <jdek@itanimul.li>
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

.macro  transpose_8x16B r0, r1, r2, r3, r4, r5, r6, r7, t0, t1
        trn1            \t0\().16b, \r0\().16b, \r1\().16b
        trn2            \t1\().16b, \r0\().16b, \r1\().16b
        trn1            \r1\().16b, \r2\().16b, \r3\().16b
        trn2            \r3\().16b, \r2\().16b, \r3\().16b
        trn1            \r0\().16b, \r4\().16b, \r5\().16b
        trn2            \r5\().16b, \r4\().16b, \r5\().16b
        trn1            \r2\().16b, \r6\().16b, \r7\().16b
        trn2            \r7\().16b, \r6\().16b, \r7\().16b

        trn1            \r4\().8h,  \r0\().8h,  \r2\().8h
        trn2            \r2\().8h,  \r0\().8h,  \r2\().8h
        trn1            \r6\().8h,  \r5\().8h,  \r7\().8h
        trn2            \r7\().8h,  \r5\().8h,  \r7\().8h
        trn1            \r5\().8h,  \t1\().8h,  \r3\().8h
        trn2            \t1\().8h,  \t1\().8h,  \r3\().8h
        trn1            \r3\().8h,  \t0\().8h,  \r1\().8h
        trn2            \t0\().8h,  \t0\().8h,  \r1\().8h

        trn1            \r0\().4s,  \r3\().4s,  \r4\().4s
        trn2            \r4\().4s,  \r3\().4s,  \r4\().4s

        trn1            \r1\().4s,  \r5\().4s,  \r6\().4s
        trn2            \r5\().4s,  \r5\().4s,  \r6\().4s

        trn2            \r6\().4s,  \t0\().4s,  \r2\().4s
        trn1            \r2\().4s,  \t0\().4s,  \r2\().4s

        trn1            \r3\().4s,  \t1\().4s,  \r7\().4s
        trn2            \r7\().4s,  \t1\().4s,  \r7\().4s
.endm

.macro  transpose_4x4H  r0, r1, r2, r3, r4, r5, r6, r7
        trn1            \r4\().4h,  \r0\().4h,  \r1\().4h
        trn2            \r5\().4h,  \r0\().4h,  \r1\().4h
        trn1            \r6\().4h,  \r2\().4h,  \r3\().4h
        trn2            \r7\().4h,  \r2\().4h,  \r3\().4h

        trn1            \r0\().2s,  \r4\().2s,  \r6\().2s
        trn2            \r2\().2s,  \r4\().2s,  \r6\().2s
        trn1            \r1\().2s,  \r5\().2s,  \r7\().2s
        trn2            \r3\().2s,  \r5\().2s,  \r7\().2s
.endm

