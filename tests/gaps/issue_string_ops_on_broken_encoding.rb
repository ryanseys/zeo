# Ruby REFUSES most String operations on a string whose bytes are not valid in
# its own encoding; zeo renders the bad bytes as U+FFFD (`StrBuf::to_utf8_lossy`,
# crates/zeo-rt/src/enc/strbuf.rs) and answers anyway.
#
# The fix is a validity gate, not an encoding one: the operations below check
# `coderange != Broken` up front and raise ArgumentError ("invalid byte
# sequence in UTF-8") or Encoding::CompatibilityError, the way CRuby's
# `rb_enc_check`/`str_enc_get` callers do. Tracked in docs/ROADMAP.md's
# lossy-UTF-8 audit.
s = "caf\xC3 x".dup.force_encoding("UTF-8")
p s.valid_encoding?

%i[upcase downcase capitalize swapcase squeeze strip rstrip].each do |m|
  begin
    s.public_send(m)
    puts "#{m}: no raise"
  rescue => e
    puts "#{m}: #{e.class}"
  end
end

begin; s.codepoints; puts "codepoints: no raise"; rescue => e; puts "codepoints: #{e.class}"; end
begin; s.count("x");  puts "count: no raise";      rescue => e; puts "count: #{e.class}"; end
begin; s.scan(/x/);   puts "scan: no raise";       rescue => e; puts "scan: #{e.class}"; end
begin; s.split(" ");  puts "split: no raise";      rescue => e; puts "split: #{e.class}"; end
begin; s.to_sym;      puts "to_sym: no raise";     rescue => e; puts "to_sym: #{e.class}"; end
