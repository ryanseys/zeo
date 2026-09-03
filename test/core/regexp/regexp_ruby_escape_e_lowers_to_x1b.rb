# Ruby's `\e` (ESC) isn't a Rust `regex`-crate escape; it must be
# translated to `\x1b`, and `#source` still shows the original `\e`.

re = /\e\[[0-9;]*m/
puts "\e[31mRED\e[0m".gsub(re, "")
puts re.source
__END__
RED
\e\[[0-9;]*m
