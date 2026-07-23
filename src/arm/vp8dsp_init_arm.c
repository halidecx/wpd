/*
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

#include <stdint.h>

#include "wpd_codec.h"
#include "vp8dsp.h"

wpd_cold void ff_vp8dsp_init_arm(VP8DSPContext *dsp)
{
    int cpu_flags = wpd_get_cpu_flags();

    if (wpd_have_armv6(cpu_flags))
        ff_vp8dsp_init_armv6(dsp);
    if (wpd_have_neon(cpu_flags))
        ff_vp8dsp_init_neon(dsp);
}
