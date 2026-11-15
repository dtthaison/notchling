use std::time::{Duration, Instant};

use super::shell::parsed_outward;

fn all_outward(cmds: &[&str]) {
    for cmd in cmds {
        assert!(parsed_outward(cmd), "{cmd} should be outward");
    }
}

fn none_outward(cmds: &[&str]) {
    for cmd in cmds {
        assert!(!parsed_outward(cmd), "{cmd} should not be outward");
    }
}

#[test]
fn git_family_table() {
    all_outward(&[
        "git push",
        "git push --force-with-lease origin main",
        "git -C /w/repo push",
        "git -c user.name=x push",
        "git send-email --to a@example.test patch",
        "/usr/bin/git push",
        "git subtree push --prefix=x origin main",
        "git lfs push origin main",
    ]);
    none_outward(&[
        "git status",
        "git pull",
        "git checkout push",
        "git stash push",
        "git commit -m 'git push later'",
        "git subtree pull --prefix=x origin main",
        "git lfs ls-files",
    ]);
}

#[test]
fn gh_family_table() {
    all_outward(&[
        "gh pr create --fill",
        "gh pr merge 12",
        "gh pr close 12",
        "gh pr comment 12 -b hi",
        "gh pr ready 3",
        "gh -R o/r pr merge 3",
        "gh release create v1",
        "gh release upload v1 a.tgz",
        "gh issue create -t x",
        "gh repo delete o/r",
        "gh run rerun 3",
        "gh workflow run x.yml",
        "gh label create x",
        "gh api -X POST repos/o/r/issues",
        "gh api --method DELETE repos/o/r/issues/1",
        "gh api -XPATCH repos/o/r/issues/1",
        "gh api repos/o/r/issues -f title=x",
        "gh api repos/o/r/issues --input body.json",
    ]);
    none_outward(&[
        "gh",
        "gh pr",
        "gh pr view 12",
        "gh pr list",
        "gh pr checkout 12",
        "gh pr diff 12",
        "gh pr checks 12",
        "gh pr status",
        "gh run watch 3",
        "gh run view 3",
        "gh run download 3",
        "gh release view v1",
        "gh release list",
        "gh release download v1",
        "gh issue list",
        "gh browse",
        "gh search repos x",
        "gh status",
        "gh auth status",
        "gh help",
        "gh version",
        "gh api repos/o/r/issues",
        "gh api -X GET repos/o/r/issues",
        "gh api --method GET repos/o/r/issues -f state=open",
    ]);
}

#[test]
fn gh_graphql_is_outward_only_for_a_plain_query() {
    all_outward(&[
        "gh api graphql -f query='mutation { addStar(input: {}) { clientMutationId } }'",
        "gh api graphql -f query='  MUTATION { x }'",
        "gh api graphql -F query=@m.graphql",
        "gh api graphql --input q.json",
        "gh api graphql -F owner=o -f query='mutation($o: String) { x }'",
        "gh api graphql -f query='query($o: String!) { repository(name: $o) { id } }'",
        "gh api graphql -f query=\"$Q\"",
    ]);
    none_outward(&[
        "gh api graphql -f query='query { viewer { login } }'",
        "gh api graphql -F owner=o -f query='{ repository { id } }'",
        "gh api graphql --field query='query { viewer { login } }'",
    ]);
}

#[test]
fn curl_get_is_not_curl_post_is() {
    none_outward(&[
        "curl https://example.test/a",
        "curl -s -o out.json https://example.test/a",
        "curl -X GET https://example.test/a",
        "curl -XHEAD https://example.test/a",
        "curl -I https://example.test/a",
        "curl --request get https://example.test/a",
        "curl -G -d q=1 https://example.test/a",
        "curl https://a --next https://b",
        "wget https://example.test/a.tgz",
        "wget --method=GET https://example.test/a",
    ]);
    all_outward(&[
        "curl -X POST https://example.test/a",
        "curl -XPOST https://example.test/a",
        "curl --request DELETE https://example.test/a",
        "curl --request=PUT https://example.test/a",
        "curl -d a=1 https://example.test/a",
        "curl -sd a=1 https://example.test/a",
        "curl --data-binary @f https://example.test/a",
        "curl --data-raw '{}' https://example.test/a",
        "curl -F file=@f https://example.test/a",
        "curl --form a=b https://example.test/a",
        "curl -T f https://example.test/a",
        "curl --upload-file f https://example.test/a",
        "curl --json '{}' https://example.test/a",
        "curl -X GET -F a=b https://example.test/a",
        "curl -K cfg https://example.test/a",
        "curl -sK cfg https://example.test/a",
        "curl --config cfg https://example.test/a",
        "curl -X POST https://a --next -X GET https://b",
        "curl https://a --next -d x https://b",
        "curl -X GET https://a -: -X PUT https://b",
        "wget --post-data=a=1 https://example.test/a",
        "wget --post-file f https://example.test/a",
        "wget --method POST https://example.test/a",
        "wget --method=PUT https://example.test/a",
    ]);
}

#[test]
fn long_flag_clusters_are_linear() {
    let started = Instant::now();
    assert!(!parsed_outward(&format!(
        "curl -{} https://example.test",
        "s".repeat(60_000)
    )));
    assert!(parsed_outward(&format!(
        "curl -{} -d x",
        "s".repeat(60_000)
    )));
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn package_publish_table() {
    all_outward(&[
        "npm publish",
        "npm publish --access public",
        "npm --prefix web publish",
        "npm run publish",
        "pnpm publish",
        "pnpm -r publish",
        "pnpm changeset publish",
        "yarn publish",
        "yarn npm publish",
        "yarn workspace web publish",
        "yarn workspace web npm publish",
        "yarn workspaces foreach -A npm publish",
        "bun publish",
        "cargo publish",
        "cargo +nightly publish -p x",
        "cargo --manifest-path c/Cargo.toml publish",
        "cargo --config x publish",
        "cargo -C dir publish",
        "cargo --target-dir t publish",
        "cargo -Z unstable-options publish",
        "docker push registry.test/app:1",
        "docker image push registry.test/app:1",
        "docker -H tcp://h:2375 push app",
    ]);
    none_outward(&[
        "npm install",
        "npm run publish-docs",
        "pnpm install",
        "yarn add left-pad",
        "yarn workspaces foreach run build",
        "bun install",
        "cargo build",
        "cargo test publish",
        "cargo --manifest-path c/Cargo.toml build",
        "docker build -t app .",
        "docker pull app",
    ]);
}

#[test]
fn docker_builds_and_compose_that_push_are_outward() {
    all_outward(&[
        "docker build --push -t a .",
        "docker buildx build --push .",
        "docker buildx bake --push",
        "docker build --output type=registry,name=x .",
        "docker buildx build -o type=image,push=true .",
        "docker buildx build --output=type=registry .",
        "docker image build --push .",
        "docker compose push",
        "docker compose -f x.yml push",
        "docker compose --project-name p --file x.yml push",
        "docker compose -p p --profile web --env-file e push",
        "docker compose build --push",
        "docker compose -f x.yml build --push",
        "docker-compose push",
        "docker-compose -f x.yml push",
        "docker-compose build --push",
    ]);
    none_outward(&[
        "docker build -t a .",
        "docker buildx build --load .",
        "docker buildx build --push=false .",
        "docker buildx build -o type=local,dest=out .",
        "docker compose up",
        "docker compose -f x.yml build",
        "docker-compose up -d",
        "docker-compose -f x.yml ps",
    ]);
}

#[test]
fn runners_are_wrappers() {
    all_outward(&[
        "npx vercel --prod",
        "npx -y vercel --prod",
        "npx --yes vercel --prod",
        "npx -p vercel vercel --prod",
        "npm exec -- vercel --prod",
        "npm x -- vercel --prod",
        "pnpm dlx vercel --prod",
        "pnpm exec vercel --prod",
        "yarn dlx vercel --prod",
        "bunx vercel --prod",
        "pnpx vercel --prod",
        "npx wrangler deploy",
        "npx firebase-tools deploy",
        "wrangler publish",
        "wrangler pages deploy dist",
        "wrangler -c w.toml deploy",
        "npx firebase deploy",
        "npx -y gh pr merge 3",
    ]);
    none_outward(&[
        "npx vercel",
        "npx prettier --write .",
        "npm exec -- tsc",
        "pnpm dlx create-vite",
        "wrangler dev",
        "wrangler pages dev dist",
        "wrangler whoami",
        "firebase-tools login",
    ]);
}

#[test]
fn notchling_secret_and_delegate_run_what_they_are_given() {
    all_outward(&[
        "notchling secret npm -- npm publish",
        "'/x/notchling' secret github -- 'curl -X POST https://a'",
        "notchling secret gh -- git push",
        "Notchling --secret a,b -- ls",
        "/Applications/Notchling.app/Contents/MacOS/Notchling --secret a -- ls",
        "notchling delegate codex -- do it",
        "notchling --delegate agy -- do it",
    ]);
    none_outward(&[
        "notchling --secrets",
        "notchling --pages",
        "notchling --page x",
    ]);
}

#[test]
fn infra_family_table() {
    all_outward(&[
        "kubectl apply -f x.yaml",
        "kubectl delete pod p",
        "kubectl rollout restart deploy/d",
        "kubectl rollout status deploy/d",
        "kubectl exec p -- ls",
        "kubectl scale deploy/d --replicas=0",
        "kubectl create ns x",
        "kubectl -n prod edit cm c",
        "kubectl -n prod apply -f x.yaml",
        "kubectl --context c1 delete pod p",
        "helm install r chart",
        "helm upgrade r chart",
        "helm uninstall r",
        "helm rollback r 1",
        "terraform apply",
        "terraform -chdir=infra apply -auto-approve",
        "terraform destroy",
        "terraform import a.b id",
        "terraform state rm a.b",
        "terraform state push f",
        "tofu apply",
        "tofu state mv a b",
        "fly deploy",
        "fly -a myapp deploy",
        "fly --app myapp deploy",
        "fly -t tok -c fly.toml -o org deploy",
        "flyctl deploy --remote-only",
        "flyctl -a myapp secrets set A=b",
        "fly secrets set A=b",
        "fly apps destroy x",
        "fly -a x scale count 2",
        "fly machine run img",
        "flyctl machines stop m",
        "firebase deploy --only hosting",
        "firebase hosting:channel:deploy preview",
        "vercel --prod",
        "vercel deploy --prod",
        "vercel --prod --yes",
    ]);
    none_outward(&[
        "kubectl",
        "kubectl get pods",
        "kubectl describe pod p",
        "kubectl -n prod get pods",
        "kubectl logs p",
        "kubectl top pods",
        "kubectl explain pod",
        "kubectl api-resources",
        "kubectl api-versions",
        "kubectl version",
        "kubectl config view",
        "kubectl diff -f x.yaml",
        "kubectl auth can-i get pods",
        "kubectl wait --for=condition=ready pod/p",
        "kubectl cluster-info",
        "kubectl port-forward p 8080:80",
        "helm list",
        "helm ls",
        "helm status r",
        "helm get values r",
        "helm history r",
        "helm template r chart",
        "helm lint chart",
        "helm show values chart",
        "helm search repo x",
        "helm repo add x https://example.test",
        "helm dependency update chart",
        "helm version",
        "helm env",
        "terraform plan",
        "terraform init",
        "terraform state list",
        "terraform state show a.b",
        "fly status",
        "fly -a myapp status",
        "fly secrets list",
        "firebase login",
        "vercel",
        "vercel dev",
        "vercel ls",
    ]);
}

#[test]
fn remote_shell_table() {
    all_outward(&[
        "ssh host ls",
        "ssh -p 22 user@host",
        "sftp host",
        "scp a.txt host:/tmp/",
        "scp host:/tmp/a.txt .",
        "rsync -a src/ host:/dst",
        "rsync -a src/ user@host:/dst",
        "rsync -a -e 'ssh -p 22' src/ host:dst",
        "rsync -a src/ rsync://host/mod",
        "rsync -a src/ host::mod",
        "rsync -a a b host:dst",
    ]);
    none_outward(&[
        "rsync -a src/ dst/",
        "rsync -a ./a:b dst/",
        "rsync -a host:/src ./local",
        "rsync -a --exclude a:b src/ dst/",
        "rsync --list-only src/",
        "rsync -a src/ dst/ > rsync.log",
    ]);
}

#[test]
fn cloud_cli_table() {
    all_outward(&[
        "aws s3 cp a s3://b/a",
        "aws s3 rm s3://b/a",
        "aws s3 sync . s3://b",
        "aws ec2 run-instances --image-id x",
        "aws --profile p lambda update-function-code --function-name f",
        "aws s3 cp list.txt s3://b/",
        "aws lambda invoke --function-name get-user out.json",
        "aws secretsmanager put-secret-value --secret-id get-x",
        "aws ec2 terminate-instances --instance-ids get-1",
        "gcloud compute instances delete i",
        "gcloud compute instances delete list-vm",
        "gcloud functions call get-fn",
        "gcloud workflows execute get-wf",
        "gcloud sql instances restart get-db",
        "gcloud compute instances list delete",
        "gcloud run deploy s",
        "gcloud --project p app deploy",
        "gcloud compute ssh get-x",
        "gcloud storage cp a gs://b",
        "gcloud config set project x",
        "az vm delete -n v",
        "az group create -n g",
        "az vm run-command invoke -n list-x",
        "az vm get-x",
        "az vm deallocate list-vm",
        "az vm restart -n x",
        "az vm start list-vm",
        "gcloud compute instances restart list-vm",
        "gcloud run jobs execute get-job",
        "gcloud auth login",
        "az login",
        "gcloud container clusters upgrade get-c",
        "az webapp restore list-x",
        "az vm resume list-vm",
        "az aks scale -n list-x",
    ]);
    none_outward(&[
        "aws s3 ls",
        "aws s3 ls s3://b/path",
        "aws ec2 describe-instances",
        "aws iam list-users",
        "aws sts get-caller-identity",
        "aws s3api list-buckets",
        "aws --profile p --region eu-west-1 s3 ls",
        "aws configure get region",
        "aws --version",
        "aws help",
        "gcloud run services list",
        "gcloud run services describe svc",
        "gcloud compute instances list",
        "gcloud compute instances describe i",
        "gcloud --project p compute instances list --filter 'name:delete'",
        "gcloud storage ls",
        "gcloud config get project",
        "gcloud config get-value project",
        "gcloud auth list",
        "gcloud info",
        "gcloud projects get-iam-policy p",
        "az vm list",
        "az vm list-sizes -l eu",
        "az group show -n g",
        "az account show",
        "az account list-locations",
    ]);
}
