# Issue #646: a top-level const assigned via `<CONST> = <Class>.
# new(...)` whose initialize body (or any method transitively
# called from initialize) reads <CONST> now raises NameError at
# runtime, matching MRI semantics.
#
# Before the fix the read returned NULL (cst_<CONST> = NULL during
# the assignment's RHS evaluation), and a subsequent `.method` or
# `->iv_` deref segfaulted. The reporter's tep#13 case hit this
# via PG::Connection.new -> Tep::Scheduler.scheduled_context? ->
# Tep::APP.sched_current.
#
# Implementation:
# * analyze records `<CONST>`'s init RHS class (when it's
#   `<Class>.new(...)`) in @const_init_class.
# * codegen emits `static int sp_init_in_progress_<CONST>` and
#   sets it around the assignment in main.
# * read sites of <CONST> wrap with `(flag ? raise_NameError :
#   cst_<CONST>)`.

class App
  def initialize
    @arr = []
    @arr << 1   # force heap allocation so App is not value-type
    begin
      _ = APP
      puts "direct: no raise"
    rescue NameError => e
      puts "direct: " + e.message
    end
    helper
  end

  def helper
    begin
      _ = APP
      puts "indirect: no raise"
    rescue NameError => e
      puts "indirect: " + e.message
    end
  end
end

APP = App.new
puts "after: APP class = " + APP.class.name
