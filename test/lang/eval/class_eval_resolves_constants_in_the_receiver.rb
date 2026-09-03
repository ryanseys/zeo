# The receiver is a class only the RUN TIME knows -- the snippet's own
# compiler has no entry for it, so its id travels as an immediate and
# every static fold stands down. The `def`'s body inherits the same
# cref, which is what makes `def tell = SECRET` find `Host::SECRET`.

class Host
  SECRET = :host_secret
end
reader = "SECRET"
p Host.class_eval(reader)
defn = "def tell = SECRET"
Host.class_eval(defn)
p Host.new.tell
p Host.private_instance_methods(false)
__END__
:host_secret
:host_secret
[]
