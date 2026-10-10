#!/usr/bin/env python3
"""CLI reporting regressions; all writable artifacts stay in the test-data dir."""

import argparse
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


class Cli(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="cli-", dir=CORPUS)
        self.addCleanup(self.temp.cleanup)
        self.work = Path(self.temp.name)
        self.input = self.work / "input.bin"
        self.input.write_bytes((CORPUS / "metadata_binary.webp").read_bytes())

    def command(self, *args, input=None, stdout=subprocess.PIPE):
        return subprocess.run([str(WPD), *map(os.fspath, args)], input=input,
                              stdout=stdout, stderr=subprocess.PIPE, timeout=30)

    def success(self, result):
        self.assertEqual(result.returncode, 0, result.stderr.decode("utf-8", "replace"))

    def test_json_uses_original_info_and_emits_once(self):
        source = CORPUS / "durations.webp"
        expected = dict(width=100, height=80, coding="unknown", is_animation=True,
                        frame_count=4, loop_count=7, has_alpha=True, has_icc=False,
                        has_exif=False, has_xmp=False,
                        durations_ms=[1, 0, 65535, 16777215])
        for stream in [[], ["--stream", "1"], ["--stream", "997"]]:
            with self.subTest(stream=stream):
                result = self.command("--info=json", "--repeat", "2", "--loops", "2",
                                      "--scale", "50x40", *stream, source)
                self.success(result)
                self.assertEqual(json.loads(result.stdout), expected)
                self.assertEqual(result.stdout.count(b"\n"), 1)
        result = self.command("--info=json", "--stream", "1", "-", input=source.read_bytes())
        self.success(result)
        self.assertEqual(json.loads(result.stdout), expected)

    def test_binary_metadata_is_exact_for_whole_stream_and_stdin(self):
        paths = [self.work / name for name in ["profile.icc", "image.exif", "image.xmp"]]
        expected = [b"ICC\0\xffprofile", b"Exif\0\0\x01\xff\x80", b"<xmp>\0\xfe</xmp>"]
        flags = [value for flag, path in zip(["--icc-out", "--exif-out", "--xmp-out"], paths)
                 for value in (flag, path)]
        for stream in [[], ["--stream", "1"], ["--stream", "997"]]:
            result = self.command("--info=json", "--repeat", "2", "--loops", "2",
                                  *flags, *stream, self.input)
            self.success(result)
            info = json.loads(result.stdout)
            self.assertEqual((info["width"], info["height"], info["durations_ms"]), (1, 1, [0]))
            self.assertTrue(all(info[key] for key in ["has_icc", "has_exif", "has_xmp"]))
            self.assertEqual([p.read_bytes() for p in paths], expected)
        result = self.command(*flags, "--stream", "1", "-", input=self.input.read_bytes())
        self.success(result)
        self.assertEqual(result.stdout, b"")
        self.assertEqual([p.read_bytes() for p in paths], expected)
        self.assertEqual(list(self.work.glob("*.part")), [])

    def test_missing_metadata_creates_empty_files(self):
        for flag in ["--icc-out", "--exif-out", "--xmp-out"]:
            path = self.work / flag
            path.write_bytes(b"old metadata")
            result = self.command(flag, path, CORPUS / "odd_lossy.webp")
            self.success(result)
            self.assertEqual(path.read_bytes(), b"")

    def test_raw_lossless_json(self):
        source = self.work / "raw.bin"
        source.write_bytes(bytes.fromhex("2f00000000071011118888fe0700"))
        result = self.command("--info=json", source)
        self.success(result)
        info = json.loads(result.stdout)
        self.assertEqual((info["width"], info["height"], info["frame_count"]), (1, 1, 1))
        self.assertFalse(info["is_animation"])

    def test_failed_decoding_and_pixel_output_leave_metadata_untouched(self):
        metadata = self.work / "profile.icc"
        for source, flags in [
            (CORPUS / "damaged" / "truncated-metadata.webp", []),
            (CORPUS / "damaged" / "truncated-lossless.webp", []),
            (self.input, ["--max-output", "1", "--muxer", "pam"]),
        ]:
            with self.subTest(source=source, flags=flags):
                metadata.write_bytes(b"preserve me")
                result = self.command("--info=json", "--icc-out", metadata,
                                      *flags, source, self.work / "pixels.pam")
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertEqual(result.stdout, b"")
                self.assertEqual(metadata.read_bytes(), b"preserve me")
                self.assertEqual(list(self.work.glob("*.part")), [])

    def test_verification_must_succeed_before_reporting(self):
        metadata = self.work / "profile.icc"
        metadata.write_bytes(b"preserve me")
        result = self.command("--info=json", "--icc-out", metadata,
                              "--verify", "0" * 32, self.input)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertEqual(result.stdout, b"")
        self.assertEqual(metadata.read_bytes(), b"preserve me")

    def test_collisions_are_rejected_before_truncating(self):
        pixel = self.work / "pixels.raw"
        alias_dir = self.work / "alias"
        alias_dir.mkdir()
        for metadata in [pixel, self.work / "." / "pixels.raw", alias_dir / ".." / "pixels.raw"]:
            pixel.write_bytes(b"preserve pixels")
            result = self.command("--icc-out", metadata, self.input, pixel)
            self.assertEqual(result.returncode, 2, result.stderr)
            self.assertEqual(pixel.read_bytes(), b"preserve pixels")
        pixel.unlink()
        result = self.command("--icc-out", pixel, self.input, pixel)
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertFalse(pixel.exists())
        original = self.input.read_bytes()
        for args in [("--icc-out", self.input, self.input),
                     ("--info=json", self.input, self.input)]:
            result = self.command(*args)
            self.assertEqual(result.returncode, 2, result.stderr)
            self.assertEqual(self.input.read_bytes(), original)
        pixel.write_bytes(b"preserve metadata")
        result = self.command("--icc-out", pixel, "--exif-out", pixel, self.input)
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertEqual(pixel.read_bytes(), b"preserve metadata")

    @unittest.skipUnless(os.name == "posix", "Unix link identities")
    def test_symlinks_and_hardlinks_cannot_hide_collisions(self):
        pixel = self.work / "pixels.raw"
        pixel.write_bytes(b"preserve pixels")
        for link in ["symlink", "hardlink"]:
            alias = self.work / link
            if link == "symlink":
                alias.symlink_to(pixel)
            else:
                os.link(pixel, alias)
            result = self.command("--icc-out", alias, self.input, pixel)
            self.assertEqual(result.returncode, 2, result.stderr)
            self.assertEqual(pixel.read_bytes(), b"preserve pixels")
            result = self.command("--icc-out", pixel, "--exif-out", alias, self.input)
            self.assertEqual(result.returncode, 2, result.stderr)
            self.assertEqual(pixel.read_bytes(), b"preserve pixels")
        target = self.work / "metadata.icc"
        target.write_bytes(b"old")
        alias = self.work / "metadata-link"
        alias.symlink_to(target)
        result = self.command("--icc-out", alias, self.input)
        self.success(result)
        self.assertTrue(alias.is_symlink())
        self.assertEqual(target.read_bytes(), b"ICC\0\xffprofile")

    def test_stdout_cannot_mix_json_and_pixels(self):
        for alias in ["-", "/dev/stdout", "/dev/fd/1", "/proc/self/fd/1"]:
            result = self.command("--info=json", self.input, alias)
            self.assertEqual(result.returncode, 2, result.stderr)
            self.assertEqual(result.stdout, b"")
            result = self.command("--icc-out", alias, self.input)
            self.assertEqual(result.returncode, 2, result.stderr)
        if os.name == "posix":
            alias = self.work / "stdout-link"
            alias.symlink_to("/dev/stdout")
            result = self.command("--info=json", self.input, alias)
            self.assertEqual(result.returncode, 2, result.stderr)
            self.assertEqual(result.stdout, b"")
            target = self.work / "stdout-file"
            target.write_bytes(b"preserve stdout")
            with target.open("ab") as stdout:
                result = self.command("--info=json", self.input, target, stdout=stdout)
            self.assertEqual(result.returncode, 2, result.stderr)
            self.assertEqual(target.read_bytes(), b"preserve stdout")

    def test_invalid_destinations_and_arguments(self):
        for args in [("--info=xml", self.input), ("--icc-out=", self.input),
                     ("--icc-out", self.work, self.input)]:
            result = self.command(*args)
            self.assertEqual(result.returncode, 2, result.stderr)
        metadata = self.work / "profile.icc"
        metadata.write_bytes(b"preserve me")
        result = self.command("--icc-out", metadata, "--exif-out", self.work / "missing" / "file", self.input)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertEqual(metadata.read_bytes(), b"preserve me")

    def test_new_case_aliases_do_not_overwrite_completed_outputs(self):
        probe = self.work / "case-probe"
        probe.write_bytes(b"probe")
        if not (self.work / "CASE-PROBE").exists():
            self.skipTest("case-sensitive filesystem")
        pixel = self.work / "pixels.raw"
        alias = self.work / "PIXELS.RAW"
        expected = self.command("--fmt", "rgba", self.input, "-")
        self.success(expected)
        result = self.command("--fmt", "rgba", "--icc-out", alias, self.input, pixel)
        self.assertNotEqual(result.returncode, 0, result.stderr)
        self.assertEqual(pixel.read_bytes(), expected.stdout)
        a, b = self.work / "metadata", self.work / "METADATA"
        result = self.command("--icc-out", a, "--exif-out", b, self.input)
        self.assertNotEqual(result.returncode, 0, result.stderr)
        self.assertEqual(a.read_bytes(), b"ICC\0\xffprofile")

    @unittest.skipUnless(os.name == "posix", "Unix file descriptors")
    def test_redirected_stdin_is_not_a_metadata_or_pixel_destination(self):
        original = self.input.read_bytes()
        for flags in [["--icc-out", str(self.input), "-"],
                      ["--info=json", "-", str(self.input)]]:
            with self.input.open("rb") as stdin:
                result = subprocess.run([str(WPD), *flags], stdin=stdin,
                                        capture_output=True, timeout=30)
            self.assertEqual(result.returncode, 2, result.stderr)
            self.assertEqual(self.input.read_bytes(), original)

    def test_unicode_metadata_filename(self):
        dest = self.work / "profile-Ω.icc"
        result = self.command("--icc-out", dest, self.input)
        self.success(result)
        self.assertEqual(dest.read_bytes(), b"ICC\0\xffprofile")

    @unittest.skipUnless(sys.platform.startswith("linux"), "Linux filename bytes; APFS requires valid Unicode")
    def test_non_utf8_metadata_path_is_preserved(self):
        name = os.fsencode(self.work) + b"/profile-\xff.icc"
        result = self.command("--icc-out", name, self.input)
        self.success(result)
        with open(name, "rb") as source:
            self.assertEqual(source.read(), b"ICC\0\xffprofile")

    def test_first_riff_chunk_is_validated_in_whole_and_split_input(self):
        for name in ["invalid-first-vp8x.webp", "invalid-first-junk.webp", "short-first-partition.webp"]:
            source = CORPUS / "damaged" / name
            for stream in [[], ["--stream", "1"], ["--stream", "19"]]:
                result = self.command("--info=json", *stream, source)
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertEqual(result.stdout, b"")

    def test_comparison_distinguishes_visible_transparent_and_geometry(self):
        sys.dont_write_bytecode = True
        path = Path(__file__).resolve().parents[1] / "scripts" / "webpcompare.py"
        spec = importlib.util.spec_from_file_location("webpcompare", path)
        compare = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(compare)
        a, b = self.work / "a.pam", self.work / "b.pam"

        def pam(width, pixels):
            return (f"P7\nWIDTH {width}\nHEIGHT 1\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n".encode() + pixels)

        a.write_bytes(pam(1, b"\x01\x02\x03\0"))
        b.write_bytes(pam(1, b"\x03\x02\x01\0"))
        self.assertEqual(compare.compare(a, b), ("transparent", 1, 0, 1))
        b.write_bytes(pam(1, b"\x03\x02\x01\xff"))
        self.assertEqual(compare.compare(a, b), ("visible", 1, 1, 0))
        b.write_bytes(pam(2, bytes(8)))
        self.assertEqual(compare.compare(a, b)[0], "geometry")
        b.write_bytes(pam(1, b"\x01"))
        with self.assertRaises(ValueError):
            compare.compare(a, b)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wpd", type=Path, required=True)
    parser.add_argument("--corpus", type=Path, required=True)
    args, remaining = parser.parse_known_args()
    WPD, CORPUS = args.wpd.resolve(), args.corpus.resolve()
    unittest.main(argv=[__file__, *remaining])
