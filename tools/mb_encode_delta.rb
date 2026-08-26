# frozen_string_literal: true

# Generates the multi-byte ENCODE deltas from the ruby 4.0.6 oracle:
#
#   crates/zeo-rt/src/enc/mb_encode_delta.rs
#
# Run as `mise exec ruby@4.0.6 -- ruby tools/mb_encode_delta.rb`. The file is
# checked in; regenerate it rather than editing by hand.
#
# zeo's multi-byte mapping comes from encoding_rs, which implements the WHATWG
# tables. Those and CRuby's disagree in the ENCODE direction, in both
# directions at once:
#
#   * Big5: WHATWG maps MORE than CRuby (the HKSCS-adjacent region, including
#     Cyrillic) and also less in places -- a deny set AND an allow table.
#   * EUC-JP: WHATWG is decode-only for JIS X 0212, so CRuby reaches ~6000
#     scalars through the SS3 plane that zeo refuses outright, plus a few
#     hundred where the two pick different bytes for the same scalar.
#
# DELTAS, not full tables: the full repertoires are 14029 + 13137 rows, and
# the disagreements are roughly a third of that. The DECODE direction needs no
# delta at all -- it already answers correctly, and every passing decode
# golden stays untouched.
#
# The generator cannot see encoding_rs, so it takes zeo's side of the
# comparison from a probe run: `ZEO=<binary> ruby tools/mb_encode_delta.rb`.
# Without `ZEO` it uses `target/release/zeo`.

ROOT = File.expand_path("..", __dir__)
ZEO = ENV["ZEO"] || File.join(ROOT, "target/release/zeo")

FAMILIES = { "Big5" => "BIG5", "EUC-JP" => "EUC_JP" }.freeze

# What an engine's encoder accepts, as `{codepoint => hex bytes}`.
PROBE = <<~RUBY
  out = []
  (0..0x10FFFF).each do |cp|
    next if cp.between?(0xD800, 0xDFFF)
    s = begin
      cp.chr(Encoding::UTF_8)
    rescue RangeError
      next
    end
    %w[Big5 EUC-JP].each do |enc|
      begin
        out << "\#{enc}\\t\#{cp}\\t\#{s.encode(enc).unpack1("H*")}"
      rescue Encoding::UndefinedConversionError
      end
    end
  end
  puts out
RUBY

# zeo reads a PROGRAM FILE, not stdin, so the probe is written out once and
# both engines are pointed at the same path.
def probe(cmd)
  require "open3"
  require "tmpdir"
  path = File.join(Dir.tmpdir, "zeo-mb-encode-probe.rb")
  File.write(path, PROBE)
  text, status = Open3.capture2(*cmd, path)
  raise "probe failed: #{cmd.inspect} (#{status.exitstatus})" unless status.success?

  by_enc = Hash.new { |h, k| h[k] = {} }
  text.each_line do |line|
    enc, cp, hex = line.chomp.split("\t")
    by_enc[enc][cp.to_i] = hex
  end
  by_enc
end

ruby = probe([RbConfig.ruby])
mine = probe([ZEO])

out = +<<~HEAD
  //! Multi-byte ENCODE deltas against encoding_rs, generated from the ruby
  //! #{RUBY_VERSION} oracle by `tools/mb_encode_delta.rb`. Do not edit by hand.
  //!
  //! encoding_rs implements the WHATWG tables, which disagree with CRuby's in
  //! the encode direction: WHATWG's Big5 maps MORE than CRuby (the
  //! HKSCS-adjacent region) and less in places, and it is decode-only for JIS
  //! X 0212, which CRuby reaches through EUC-JP's SS3 plane.
  //!
  //! DELTAS rather than full tables -- the full repertoires are three times
  //! the size -- and ENCODE only: the decode direction already agrees, so
  //! every passing decode golden is untouched.
  //!
  //! `DENY` is consulted first (the scalar is refused however the backend
  //! maps it), then `ALLOW` (these bytes win), then the backend.

HEAD

FAMILIES.each do |enc, konst|
  deny = (mine[enc].keys - ruby[enc].keys).sort
  # An ALLOW row is one ruby maps and zeo does not, OR one where the two
  # disagree about the bytes -- the second kind is a correction, and reads
  # exactly the same way at the lookup.
  allow = ruby[enc].select { |cp, hex| mine[enc][cp] != hex }.sort
  out << "/// #{enc} scalars CRuby refuses and the WHATWG table maps.\n"
  out << "pub(crate) const #{konst}_DENY: &[char] = &[\n"
  deny.each { |cp| out << format("    '\\u{%x}',\n", cp) }
  out << "];\n\n"
  out << "/// #{enc} scalars whose CRuby bytes the WHATWG table does not give.\n"
  out << "pub(crate) const #{konst}_ALLOW: &[(char, &[u8])] = &[\n"
  allow.each do |cp, hex|
    bytes = hex.scan(/../).map { |b| format("0x%s", b) }.join(", ")
    out << format("    ('\\u{%x}', &[%s]),\n", cp, bytes)
  end
  out << "];\n\n"
  warn "#{enc}: #{deny.size} deny, #{allow.size} allow"
end

File.write(File.join(ROOT, "crates/zeo-rt/src/enc/mb_encode_delta.rs"), out)
puts "wrote crates/zeo-rt/src/enc/mb_encode_delta.rs"
