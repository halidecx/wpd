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

function ff_pred16x16_hor_neon, export=1
        sub             x2,  x0,  #1
        mov             w3,  #16
1:      ld1r            {v0.16b}, [x2], x1
        subs            w3,  w3,  #1
        st1             {v0.16b}, [x0], x1
        b.ne            1b
        ret
endfunc

function ff_pred16x16_vert_neon, export=1
        sub             x2,  x0,  x1
        add             x1,  x1,  x1
        ld1             {v0.16b}, [x2], x1
        mov             w3,  #8
1:      subs            w3,  w3,  #1
        st1             {v0.16b}, [x0], x1
        st1             {v0.16b}, [x2], x1
        b.ne            1b
        ret
endfunc
