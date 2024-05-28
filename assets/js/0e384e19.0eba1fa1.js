"use strict";(self.webpackChunkwebsite=self.webpackChunkwebsite||[]).push([[976],{1512:(e,n,l)=>{l.r(n),l.d(n,{assets:()=>c,contentTitle:()=>s,default:()=>h,frontMatter:()=>t,metadata:()=>d,toc:()=>o});var r=l(4848),a=l(8453);const t={id:"intro",title:"Try Cala",slug:"/"},s="Run locally",d={id:"intro",title:"Try Cala",description:"Install Docker",source:"@site/docs/intro.md",sourceDirName:".",slug:"/",permalink:"/docs/",draft:!1,unlisted:!1,editUrl:"https://github.com/GaloyMoney/cala/edit/main/docs/intro.md",tags:[],version:"current",frontMatter:{id:"intro",title:"Try Cala",slug:"/"},sidebar:"demoSidebar",next:{title:"Create journal and accounts",permalink:"/docs/docs/demo/create-journal-and-accounts"}},c={},o=[{value:"Install Docker",id:"install-docker",level:2},{value:"Download and install the cala-server binary",id:"download-and-install-the-cala-server-binary",level:2},{value:"Run the Cala server",id:"run-the-cala-server",level:2},{value:"Start the dependencies in Docker",id:"start-the-dependencies-in-docker",level:3},{value:"Start the server",id:"start-the-server",level:3},{value:"GraphQL demo",id:"graphql-demo",level:2},{value:"Cleanup",id:"cleanup",level:2}];function i(e){const n={a:"a",code:"code",h1:"h1",h2:"h2",h3:"h3",li:"li",pre:"pre",ul:"ul",...(0,a.R)(),...e.components};return(0,r.jsxs)(r.Fragment,{children:[(0,r.jsx)(n.h1,{id:"run-locally",children:"Run locally"}),"\n",(0,r.jsx)(n.h2,{id:"install-docker",children:"Install Docker"}),"\n",(0,r.jsxs)(n.ul,{children:["\n",(0,r.jsxs)(n.li,{children:["choose the install method for your system ",(0,r.jsx)(n.a,{href:"https://docs.docker.com/desktop/",children:"https://docs.docker.com/desktop/"})]}),"\n"]}),"\n",(0,r.jsx)(n.h2,{id:"download-and-install-the-cala-server-binary",children:"Download and install the cala-server binary"}),"\n",(0,r.jsx)(n.pre,{children:(0,r.jsx)(n.code,{children:"# download\nwget https://github.com/GaloyMoney/cala/releases/download/0.1.3/cala-server-x86_64-unknown-linux-musl-0.1.3.tar.gz\n\n# unpack\ntar -xvf cala-server-x86_64-unknown-linux-musl-0.1.3.tar.gz\n\n# add to path\nPATH=$PATH:$PWD/cala-server-x86_64-unknown-linux-musl-0.1.3\n"})}),"\n",(0,r.jsx)(n.h2,{id:"run-the-cala-server",children:"Run the Cala server"}),"\n",(0,r.jsx)(n.h3,{id:"start-the-dependencies-in-docker",children:"Start the dependencies in Docker"}),"\n",(0,r.jsx)(n.pre,{children:(0,r.jsx)(n.code,{children:"git clone https://github.com/GaloyMoney/cala\ncd cala\nmake start-deps\n"})}),"\n",(0,r.jsx)(n.h3,{id:"start-the-server",children:"Start the server"}),"\n",(0,r.jsx)(n.pre,{children:(0,r.jsx)(n.code,{children:"cala-server --config ./bats/cala.yml postgres://user:password@127.0.0.1:5432/pg\n"})}),"\n",(0,r.jsx)(n.h2,{id:"graphql-demo",children:"GraphQL demo"}),"\n",(0,r.jsxs)(n.ul,{children:["\n",(0,r.jsxs)(n.li,{children:["open the local GraphQL playground: ",(0,r.jsx)("br",{}),"\n",(0,r.jsx)(n.a,{href:"http://localhost:2252",children:"http://localhost:2252"})]}),"\n",(0,r.jsxs)(n.li,{children:["continue with the ",(0,r.jsx)(n.a,{href:"docs/demo/create-journal-and-accounts",children:"GraphQL API demo"})]}),"\n"]}),"\n",(0,r.jsx)(n.h2,{id:"cleanup",children:"Cleanup"}),"\n",(0,r.jsx)(n.pre,{children:(0,r.jsx)(n.code,{children:"make clean-deps\n"})})]})}function h(e={}){const{wrapper:n}={...(0,a.R)(),...e.components};return n?(0,r.jsx)(n,{...e,children:(0,r.jsx)(i,{...e})}):i(e)}}}]);