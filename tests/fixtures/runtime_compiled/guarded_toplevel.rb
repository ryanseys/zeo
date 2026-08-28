if ENV["ZEO_A_NAME_NOTHING_SETS"]
  def guarded_choice = :then_branch
  GUARDED_WHICH = :then_branch
else
  def guarded_choice = :else_branch
  GUARDED_WHICH = :else_branch
end

class GuardedHolder
  def which = guarded_choice
end
