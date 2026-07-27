# net/smtp is a pure-Ruby default gem (no C extension) that isn't vendored
# under gems/ -- `require "net/smtp"` raises LoadError.
require "net/smtp"
p defined?(Net::SMTP)
