// Windows-native system layer: SCM, IP Helper, process APIs, elevation, winget,
// and the sysinfo primitives the Doctor probes use. Direct Win32 calls (no
// sc.exe/netstat/parsing shims) so there's no command-injection surface and no
// locale-fragile text output for core reads. WinGet is the one subprocess —
// its stable text output is parsed by pure functions with fixture tests.

pub mod elevate;
pub mod net;
pub mod scm;
pub mod sys;
pub mod winget;

/// Curated dev-service keywords used to decide which Windows services are
/// "noteworthy" (seeded into the Services tab and badged in the sidebar).
/// Matched case-insensitively against both the service key name and its
/// display name.
const NOTEWORTHY_KEYWORDS: &[&str] = &[
    "mysql",
    "maria",
    "postgres",
    "redis",
    "mongo",
    "elastic",
    "docker",
    "nginx",
    "apache",
    "httpd",
    "memcache",
    "rabbit",
    "kafka",
    "zookeeper",
    "cassandra",
    "minio",
    "kong",
    "solr",
    "tomcat",
    "oracle",
    "mssql",
    "sql server",
    "clickhouse",
    "influx",
    "etcd",
    "consul",
    "vault",
    "neo4j",
    "couchbase",
    "bitnami",
    "php",
    "haproxy",
    "grafana",
    "prometheus",
];

pub fn noteworthy(name: &str, display_name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let display = display_name.to_ascii_lowercase();
    NOTEWORTHY_KEYWORDS
        .iter()
        .any(|k| name.contains(k) || display.contains(k))
}

/// A service that is expected to bind a TCP port when healthy — the port-presence
/// health signal the Services tab renders (started-but-unreachable → "starting").
/// Services not in this list report healthy whenever the SCM says they're running.
pub fn expects_port(name: &str, display_name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let display = display_name.to_ascii_lowercase();
    const PORT_EXPECTERS: &[&str] = &[
        "mysql",
        "maria",
        "postgres",
        "redis",
        "mongo",
        "elastic",
        "memcache",
        "rabbit",
        "kafka",
        "cassandra",
        "clickhouse",
        "influx",
        "neo4j",
        "couchbase",
        "nginx",
        "apache",
        "httpd",
        "haproxy",
        "kong",
        "solr",
        "tomcat",
        "minio",
        "grafana",
        "prometheus",
    ];
    PORT_EXPECTERS
        .iter()
        .any(|k| name.contains(k) || display.contains(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noteworthy_matches_dev_services_only() {
        assert!(noteworthy("MySQL80", "MySQL"));
        assert!(noteworthy("postgresql-x64-16", "PostgreSQL Server 16"));
        assert!(noteworthy("Redis", "Redis"));
        assert!(noteworthy(
            "Docker Desktop Service",
            "Docker Desktop Service"
        ));
        assert!(!noteworthy("Spooler", "Print Spooler"));
        assert!(!noteworthy("WinDefend", "Microsoft Defender Antivirus"));
    }

    #[test]
    fn port_health_only_for_listeners() {
        assert!(expects_port("MySQL80", "MySQL"));
        assert!(expects_port("redis", "Redis"));
        assert!(!expects_port("Spooler", "Print Spooler"));
    }
}
