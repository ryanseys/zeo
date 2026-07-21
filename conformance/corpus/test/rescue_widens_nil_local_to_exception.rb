def f
  raised = nil
  begin
    raise NotImplementedError, "boom"
  rescue NotImplementedError => e
    raised = e
  end
  raised
end

puts f.inspect
