# net/ftp is a pure-Ruby default gem (no C extension) that isn't vendored
# under gems/ -- `require "net/ftp"` raises LoadError.
require "net/ftp"
p defined?(Net::FTP)
