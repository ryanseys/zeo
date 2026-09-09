# Both name the directory the handle was opened on.
# (spinel issue #3250)
p(Dir.new(".").inspect)
p(Dir.new(".").to_path)
d = Dir.new(".")
puts d.inspect
__END__
"#<Dir:.>"
"."
#<Dir:.>
