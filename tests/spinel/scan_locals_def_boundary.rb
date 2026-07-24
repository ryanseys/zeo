# #450 cascade 1. `scan_locals` walked into nested DefNode bodies
# from any outer-context caller (notably infer_main_call_types' top-
# level scan), pulling locals from inside unrelated methods into the
# enclosing scope. Two methods that each defined `local_name = ...`
# with disagreeing inferred types then collided via scan_locals'
# repeated-write merge and widened the conflated slot to poly.
# The poly local then poisoned every call-site arg type read of
# that local name, propagating into callee params via the module-
# dispatch widening arms in scan_new_calls.
#
# Real-world repro shape (after roundhouse-style boil-down):
#
#   module CgiIo
#     def self.parse_request(env, stdin)
#       path = env["PATH_INFO"] || "/"   # path local : nullable-via-||
#       { path: path }
#     end
#   end
#
#   module Db
#     def self.configure(path); ...; end  # path : should be string
#   end
#
#   module Main
#     def self.run_setup
#       path = "/tmp/db"
#       Db.configure(path)               # this call's arg was poly
#                                        # because of cgi_io.parse_request's
#                                        # path-local leakage
#     end
#   end
#
# Pin both shapes here so a future scan_locals regression that
# re-introduces def-boundary crossing fails immediately.

module CgiIo
  def self.parse_request(env)
    path = env["PATH_INFO"] || "/"
    { path: path }
  end
end

module Db
  def self.configure(path)
    "opening " + path
  end
end

module Main
  def self.run_setup
    db_path = "/tmp/db"
    path = (!db_path.nil? && !db_path.empty?) ? db_path : ":memory:"
    Db.configure(path)
  end
end

puts CgiIo.parse_request({ "PATH_INFO" => "/articles" })[:path]
puts Main.run_setup
