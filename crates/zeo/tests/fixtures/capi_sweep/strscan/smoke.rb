require "strscan"

s = StringScanner.new("3.14 abc def")
p s.scan(/\d+/), s.scan(/\./), s.scan(/\d+/), s.skip(/\s+/), s.check(/\w+/), s.pos, s.rest, s.eos?
p s.scan(/(a)(b)(c)/), s[1], s[2], s.matched, s.pre_match, s.post_match
p s.scan_until(/e/), s.getch, s.peek(1), s.rest_size
s.unscan
p s.pos, s.scan(/f/), s.eos?
p StringScanner.new("ab").scan_until(/(?<x>b)/), StringScanner.new("xyz").search_full(/y/, false, true)
