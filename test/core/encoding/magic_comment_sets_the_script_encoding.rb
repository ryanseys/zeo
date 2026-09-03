# encoding: ISO-8859-1
# A `# encoding:` comment on the first line tags every string literal and
# __ENCODING__ with that encoding. Verified against ruby 4.0.6. (The
# source must start with the comment, so no leading newline here.)

p __ENCODING__
p "hi".encoding
p "hi".encoding == Encoding::ISO_8859_1
p "hi".bytes
__END__
#<Encoding:ISO-8859-1>
#<Encoding:ISO-8859-1>
true
[104, 105]
