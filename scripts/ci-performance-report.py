"""Export Actions job/step wall times without confusing runner minutes with elapsed time."""
import argparse
from datetime import datetime
import json
import math
import os
from pathlib import Path
import urllib.request


def seconds(start, end):
    return max(0, (datetime.fromisoformat(end) - datetime.fromisoformat(start)).total_seconds()) if start and end else 0


def summarize(run, jobs):
    active = [job for job in jobs if job["conclusion"] != "skipped" and job.get("started_at")]
    output = {"run_id": run["id"], "url": run["html_url"], "commit": run["head_sha"],
              "event": run["event"], "conclusion": run["conclusion"],
              "complete": run["status"] == "completed",
              "workflow_wall_seconds": seconds(run["run_started_at"], max(
                  (job["completed_at"] for job in active if job.get("completed_at")), default=None)),
              "runner_seconds": sum(seconds(job["started_at"], job["completed_at"]) for job in active),
              "runner_minutes_rounded_per_job": sum(math.ceil(seconds(job["started_at"], job["completed_at"]) / 60) for job in active),
              "billing_scope": "Observed job wall time; per-job rounding estimate, excludes unknown billing plan, discounts and rates",
              "cache_scope": "Classify from logs; warm/cold is not inferred from a successful run",
              "jobs": []}
    for job in jobs:
        output["jobs"].append({"id": job["id"], "name": job["name"], "conclusion": job["conclusion"],
                               "started_at": job["started_at"], "completed_at": job["completed_at"],
                               "seconds": seconds(job["started_at"], job["completed_at"]),
                               "steps": [{"name": step["name"], "conclusion": step["conclusion"],
                                          "seconds": seconds(step["started_at"], step["completed_at"])} for step in job["steps"]]})
    return output


def fetch(path):
    headers = {"Accept": "application/vnd.github+json", "User-Agent": "aether-ci-performance"}
    token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
    if token:
        headers["Authorization"] = "Bearer " + token
    request = urllib.request.Request("https://api.github.com/" + path, headers=headers)
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", default="maelmoreau21/Aether")
    parser.add_argument("--run", type=int, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    base = f"repos/{args.repository}/actions/runs/{args.run}"
    run = fetch(base)
    jobs = []
    page = 1
    while True:
        batch = fetch(f"{base}/jobs?per_page=100&page={page}")["jobs"]
        jobs.extend(batch)
        if len(batch) < 100:
            break
        page += 1
    result = summarize(run, jobs)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({key: value for key, value in result.items() if key != "jobs"}, ensure_ascii=True))


if __name__ == "__main__":
    main()
