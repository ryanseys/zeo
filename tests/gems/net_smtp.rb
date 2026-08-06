# net/smtp vendored. Its whole class-method surface is written `def SMTP.name`,
# and `default_ssl_port` is a `class << self` alias of one of them.
require "net/smtp"

p Net::SMTP.default_port
p Net::SMTP.default_submission_port
p Net::SMTP.default_tls_port
p Net::SMTP.default_ssl_port
p Net::SMTP.ancestors.include?(Net::Protocol)

smtp = Net::SMTP.new("mail.example.test", 2525)
p smtp.address, smtp.port
p smtp.started?
p smtp.open_timeout, smtp.read_timeout
p smtp.esmtp?
p smtp.tls?, smtp.starttls?

# The error hierarchy comes from net/protocol.
p Net::SMTPAuthenticationError.ancestors.include?(Net::ProtoAuthError)
p Net::SMTPServerBusy.ancestors.include?(Net::ProtoServerError)
p Net::SMTPSyntaxError.ancestors.include?(Net::ProtoSyntaxError)
p Net::SMTPFatalError.ancestors.include?(Net::ProtoFatalError)
