require "strscan"

def t(label)
  print label, ": "
  p yield
rescue => e
  puts "#{e.class}: #{e.message}"
end

s = StringScanner.new("This is an example string")
t("bol0") { s.beginning_of_line? }
t("eos0") { s.eos? }
t("scan1") { s.scan(/\w+/) }
t("scan2") { s.scan(/\w+/) }
t("scan3") { s.scan(/\s+/) }
t("pre") { s.pre_match }
t("post") { s.post_match }
t("matched") { s.matched }
t("msize") { s.matched_size }
t("pos") { s.pos }
t("charpos") { s.charpos }
t("bol") { s.beginning_of_line? }
t("rest") { s.rest }
t("restq") { s.rest? }
t("unscan") { s.unscan; [s.pos, s.matched] }
t("skip") { s.skip(/\s+is\s+/) }
t("checku") { s.check_until(/example/) }
t("posu") { s.pos }
t("scanu") { s.scan_until(/example/) }
t("preu") { s.pre_match }
t("postu") { s.post_match }
t("exist") { s.exist?(/str/) }
t("skipu") { s.skip_until(/str/) }
t("m?") { s.match?(/ing/) }
t("check") { s.check(/ing/) }
t("getch") { s.getch }
t("getbyte") { s.get_byte }
t("scanbyte") { s.scan_byte }
t("peek") { s.peek(20) }
t("peekb") { s.peek_byte }
t("eos") { s.eos? }
t("scaneos") { s.scan(/x/) }
t("terminate") { s.terminate; [s.pos, s.eos?] }
t("reset") { s.reset; [s.pos, s.matched?] }

# String patterns leave real match state.
t("strscan") { s.scan("This") }
t("strm") { [s.matched, s.pre_match, s.post_match, s[0], s.size] }
t("strmiss") { s.scan("nope") }
t("strafter") { s.matched? }
t("struntil") { s.scan_until(" an") }
t("strupre") { s.pre_match }

# Captures and names.
s2 = StringScanner.new("Fri Dec 12 1975")
t("caps") { s2.scan(/(?<wday>\w+) (?<month>\w+) (?<day>\d+)/) }
t("bracket0") { s2[0] }
t("name") { [s2[:wday], s2["month"]] }
t("named") { s2.named_captures }
t("capsarr") { s2.captures }
t("vals") { s2.values_at(0, 2, -1) }
t("sizec") { s2.size }
t("rangeidx") { s2[0..1] }

# Anchoring semantics: \A means the position in default mode.
s3 = StringScanner.new("abc def")
s3.pos = 4
t("anchorA") { s3.scan(/\Adef/) }
t("caret") { s3.scan(/^def/) }
s3.pos = 3
t("caret_mid") { s3.scan(/^\s/) }

# fixed_anchor: \A means the string head; \G the position.
f = StringScanner.new("abc def", fixed_anchor: true)
f.pos = 4
t("fixedA") { f.scan(/\Adef/) }
t("fixedG") { f.scan(/def/) }
t("fixedpre") { f.pre_match }
t("fixedpost") { f.post_match }

# Multibyte: positions are bytes.
u = StringScanner.new("héllo wörld")
t("uscan") { u.scan(/h\Sllo/) }
t("upos") { u.pos }
t("ucharpos") { u.charpos }
t("upeek") { u.peek(3) }
t("uget") { u.scan(/\s/) && u.getch }
t("upos2") { [u.pos, u.charpos] }

# Growth and replacement.
g = StringScanner.new(+"ab")
t("concat") { g << "cd"; g.string }
t("seteq") { g.string = "xyz"; [g.pos, g.string] }
t("posneg") { g.pos = -2; g.pos }
t("posrange") { g.pos = 99 }

# Errors.
e = StringScanner.new("q")
t("unscanerr") { e.unscan }
t("peekneg") { e.peek(-1) }
t("scaninty") { StringScanner.new("123abc").scan_integer }
t("scanint16") { StringScanner.new("0xffz", fixed_anchor: false).scan_integer(base: 16) }
t("inspect0") { StringScanner.new("test string").inspect }
t("inspectmid") { w = StringScanner.new("test string"); w.scan(/test /); w.inspect }
t("inspectfin") { w = StringScanner.new("t"); w.scan(/t/); w.inspect }
