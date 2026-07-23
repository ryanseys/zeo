//! `Etc` -- the native `ext/etc` module (system user/group databases,
//! `sysconf`/`confstr`, `uname`, `nprocessors`, install-path helpers).
//!
//! Host-specific values (the actual uid, hostname, cpu count) can't be pinned,
//! so these assert the CONTRACT: types, round-trips, member sets, and the
//! Struct-like surface of `Etc::Passwd`/`Etc::Group`.

use crate::support::run_ruby;

#[test]
fn module_paths_and_processor_count() {
    let result = run_ruby(
        r#"
        require "etc"
        puts Etc.sysconfdir.class
        puts Etc.systmpdir.class
        puts Etc.nprocessors.class
        puts Etc.nprocessors > 0
        puts(Etc.getlogin.is_a?(String) || Etc.getlogin.nil?)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "String\nString\nInteger\ntrue\ntrue\n");
}

#[test]
fn passwd_lookup_types_members_and_roundtrip() {
    let result = run_ruby(
        r#"
        require "etc"
        pw = Etc.getpwuid            # current effective uid
        puts pw.class
        puts pw.is_a?(Etc::Passwd)
        puts pw.name.is_a?(String)
        puts pw.uid.is_a?(Integer)
        puts pw.gid.is_a?(Integer)
        puts pw.dir.is_a?(String)
        puts pw.shell.is_a?(String)
        # getpwnam(name) round-trips back to the same uid.
        puts(Etc.getpwnam(pw.name).uid == pw.uid)
        # getpwuid(uid) round-trips back to the same name.
        puts(Etc.getpwuid(pw.uid).name == pw.name)
        # Struct-like surface.
        puts pw.members.first(4).inspect
        puts pw.to_a.length == pw.members.length
        puts pw.to_h[:uid] == pw.uid
        puts pw[:name] == pw.name
        puts pw[0] == pw.name
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Etc::Passwd\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\n[:name, :passwd, :uid, :gid]\ntrue\ntrue\ntrue\ntrue\n"
    );
}

#[test]
fn group_lookup_types_and_roundtrip() {
    let result = run_ruby(
        r#"
        require "etc"
        gr = Etc.getgrgid           # current effective gid
        puts gr.class
        puts gr.is_a?(Etc::Group)
        puts gr.name.is_a?(String)
        puts gr.gid.is_a?(Integer)
        puts gr.mem.is_a?(Array)
        puts(Etc.getgrnam(gr.name).gid == gr.gid)
        puts gr.members.inspect
        puts gr.to_h.key?(:mem)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Etc::Group\ntrue\ntrue\ntrue\ntrue\ntrue\n[:name, :passwd, :gid, :mem]\ntrue\n"
    );
}

#[test]
fn uname_returns_the_five_fields() {
    let result = run_ruby(
        r#"
        require "etc"
        u = Etc.uname
        puts u.class
        puts u.keys.sort.inspect
        puts u.values.all? { |v| v.is_a?(String) }
        puts u[:sysname].empty? == false
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Hash\n[:machine, :nodename, :release, :sysname, :version]\ntrue\ntrue\n"
    );
}

#[test]
fn sysconf_confstr_and_constants() {
    let result = run_ruby(
        r#"
        require "etc"
        puts Etc::SC_CLK_TCK.is_a?(Integer)
        puts Etc::SC_OPEN_MAX.is_a?(Integer)
        puts Etc::SC_NPROCESSORS_ONLN.is_a?(Integer)
        puts Etc::PC_NAME_MAX.is_a?(Integer)
        puts Etc::CS_PATH.is_a?(Integer)
        puts Etc.sysconf(Etc::SC_CLK_TCK) > 0
        puts Etc.sysconf(Etc::SC_OPEN_MAX) > 0
        puts(Etc.confstr(Etc::CS_PATH).is_a?(String) || Etc.confstr(Etc::CS_PATH).nil?)
        # nprocessors agrees with the sysconf value.
        puts(Etc.nprocessors == Etc.sysconf(Etc::SC_NPROCESSORS_ONLN))
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\n");
}

#[test]
fn a_missing_user_or_group_raises_argument_error() {
    let result = run_ruby(
        r#"
        require "etc"
        begin
          Etc.getpwnam("no_such_user_zqxjkw")
        rescue ArgumentError => e
          puts "user: #{e.class}"
        end
        begin
          Etc.getgrnam("no_such_group_zqxjkw")
        rescue ArgumentError => e
          puts "group: #{e.class}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "user: ArgumentError\ngroup: ArgumentError\n");
}

#[test]
fn passwd_database_cursor_and_block_iteration() {
    let result = run_ruby(
        r#"
        require "etc"
        # The block form iterates the whole database; every row is an Etc::Passwd
        # and root (uid 0) is always present.
        count = 0
        saw_root = false
        Etc.passwd do |p|
          count += 1
          saw_root = true if p.uid == 0
        end
        puts count > 0
        puts saw_root
        # setpwent rewinds; getpwent then returns a row (or nil at the end).
        Etc.setpwent
        first = Etc.getpwent
        puts(first.nil? || first.is_a?(Etc::Passwd))
        Etc.endpwent
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\ntrue\n");
}
