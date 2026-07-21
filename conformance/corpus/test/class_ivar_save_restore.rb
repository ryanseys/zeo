# A capture-style save/restore of a class-ivar whose type widens to poly under
# whole-program pressure. @query_log is written a bare `[]` (poly-array), pushed
# strings (str-array), and restored from `prev`; those heterogeneous writes merge
# the slot to poly. The `prev = @query_log` local must widen to poly with it --
# otherwise codegen emits an unsound `sp_StrArray *` <- `sp_RbVal` (issue #1793).
class Db
  @query_log = nil
  def self.capture_sql
    prev = @query_log
    log = []
    @query_log = log
    begin
      yield
    ensure
      @query_log = prev
    end
    log
  end
  def self.record_query(sql)
    @query_log.push(sql) unless @query_log.nil?
  end
end

class ArticlesTest
  def test_index
    sql = Db.capture_sql { Db.record_query("SELECT 1"); Db.record_query("SELECT 2") }
    sql.length
  end
end
p ArticlesTest.new.test_index
