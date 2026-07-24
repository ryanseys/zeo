# Uninitialised constant reads raise NameError instead of
# silently returning 0.

begin
  puts NONEXISTENT_CONST
rescue NameError
  puts "uninit raised"
end

# Self-referential init `X = X + 1` reads X before X is bound;
# CRuby raises NameError. Spinel previously evaluated X as 0
# and assigned 1.
begin
  X = X + 1
  puts X
rescue NameError
  puts "self-ref raised"
end
