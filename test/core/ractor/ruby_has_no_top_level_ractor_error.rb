# A top-level `RactorError` constant.
p defined?(RactorError)
p Object.constants.include?(:RactorError)
p Ractor.constants.sort
__END__
nil
false
[:ClosedError, :Error, :IsolationError, :MovedError, :MovedObject, :Port, :RemoteError, :UnsafeError]
