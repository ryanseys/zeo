# An exception carries its fully qualified Ruby name at run time, but a
# `when Mod::Klass` pattern node holds only the last segment. Comparing that
# directly never matched, so the branch was silently skipped -- the case fell
# through to no arm at all rather than mismatching visibly.
module App
  class HelpRequested < RuntimeError; end
  class BadUsage < App::HelpRequested; end
  class Unrelated < RuntimeError; end
end

def classify(e)
  case e
  when App::BadUsage then "usage"
  when App::HelpRequested then "help"
  when App::Unrelated then "other"
  else "none"
  end
end

begin; raise App::HelpRequested; rescue => e; puts classify(e); end
begin; raise App::BadUsage;      rescue => e; puts classify(e); end
begin; raise App::Unrelated;     rescue => e; puts classify(e); end
begin; raise RuntimeError;       rescue => e; puts classify(e); end

# the subclass still matches its qualified ancestor
begin
  raise App::BadUsage
rescue App::HelpRequested => e
  puts e.is_a?(App::HelpRequested)
  puts e.class.to_s
end

# a case directly on the rescue-captured variable (the reported shape)
$events = []
begin
  raise App::HelpRequested
rescue App::HelpRequested => e
  case e
  when App::HelpRequested then $events << "help"
  end
end
p $events
