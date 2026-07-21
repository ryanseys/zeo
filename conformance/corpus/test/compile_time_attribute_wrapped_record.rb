Record = Struct.new(:published)

class WrappedResult
  attributes :ok
  attributes :status

  def initialize(ok, status, record_present, record)
    @ok = ok
    @status = status
    @record_present = record_present
    @record = record
  end

  def ok?
    @ok
  end

  def record
    return nil if @record_present == 0

    @record
  end
end

class ActionResult
  attributes :status
  attributes :error

  def initialize(status, record_present, record, error)
    @status = status
    @record_present = record_present
    @record = record
    @error = error
  end

  def record
    return nil if @record_present == 0

    @record
  end
end

class ActionContext
  def ok(record)
    ActionResult.new(200, 1, record, "")
  end

  def error(status, message)
    ActionResult.new(status, 0, Record.new(""), message)
  end
end

class ManualAction
  def self.call(input, context)
    context.ok(Record.new("manual:reviewed"))
  end
end

class Domain
  def manual_publish
    action_result = ManualAction.call(0, ActionContext.new)
    if action_result.status >= 400 || action_result.error.length > 0
      return WrappedResult.new(false, action_result.status, 0, Record.new(""))
    end

    WrappedResult.new(true, action_result.status, 1, action_result.record)
  end
end

result = Domain.new.manual_publish
puts result.ok?
puts result.record.published
