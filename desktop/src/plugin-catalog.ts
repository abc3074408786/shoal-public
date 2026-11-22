export const pluginCategories=['开发协作','文件设计','消息日程','业务工具','生活服务'] as const;
export type PublicPlugin={id:string;name:string;description:string;icon:string;color:string;url:string;category:typeof pluginCategories[number]};
export const publicPlugins:PublicPlugin[]=[
  {id:'github',name:'GitHub',description:'处理 PR、Issue、CI 与发布流程',icon:'github',color:'#181717',url:'https://github.com/github/github-mcp-server',category:'开发协作'},
  {id:'slack',name:'Slack',description:'搜索和管理团队消息',icon:'slack',color:'#611F69',url:'https://slack.com/',category:'开发协作'},
  {id:'google-drive',name:'Google Drive',description:'云端硬盘、文档、表格与演示文稿',icon:'googledrive',color:'#0F9D58',url:'https://drive.google.com/',category:'文件设计'},
  {id:'adobe',name:'Adobe',description:'设计、组合与编辑创意内容',icon:'adobe',color:'#EB1000',url:'https://www.adobe.com/',category:'文件设计'},
  {id:'dropbox',name:'Dropbox',description:'查找、创建和管理文件',icon:'dropbox',color:'#0061FF',url:'https://www.dropbox.com/',category:'文件设计'},
  {id:'canva',name:'Canva',description:'创建、审阅和编辑设计',icon:'canva',color:'#00A5B9',url:'https://www.canva.com/',category:'文件设计'},
  {id:'gmail',name:'Gmail',description:'读取和管理邮件',icon:'gmail',color:'#EA4335',url:'https://mail.google.com/',category:'消息日程'},
  {id:'google-calendar',name:'Google Calendar',description:'管理日历与日程安排',icon:'googlecalendar',color:'#4285F4',url:'https://calendar.google.com/',category:'消息日程'},
  {id:'outlook',name:'Outlook Email',description:'整理和管理 Outlook 收件箱',icon:'microsoftoutlook',color:'#0078D4',url:'https://outlook.live.com/',category:'消息日程'},
  {id:'icloud-mail',name:'iCloud Mail',description:'读取和管理 iCloud 邮件',icon:'mail',color:'#079AFF',url:'https://www.icloud.com/mail',category:'消息日程'},
  {id:'icloud-calendar',name:'iCloud Calendar',description:'查看和管理 iCloud 日历',icon:'calendar',color:'#EF4444',url:'https://www.icloud.com/calendar',category:'消息日程'},
  {id:'quickbooks',name:'Intuit QuickBooks',description:'管理财务、付款和薪资',icon:'quickbooks',color:'#2CA01C',url:'https://quickbooks.intuit.com/',category:'业务工具'},
  {id:'hubspot',name:'HubSpot',description:'连接客户资料与业务行动',icon:'hubspot',color:'#FF7A59',url:'https://www.hubspot.com/',category:'业务工具'},
  {id:'stripe',name:'Stripe',description:'管理收款与业务收入',icon:'stripe',color:'#635BFF',url:'https://stripe.com/',category:'业务工具'},
  {id:'gusto',name:'Gusto',description:'管理员工与薪资事务',icon:'gusto',color:'#F45D48',url:'https://gusto.com/',category:'业务工具'},
  {id:'tripsy',name:'Tripsy',description:'规划和整理旅行行程',icon:'luggage',color:'#F59E0B',url:'https://tripsy.app/',category:'生活服务'},
  {id:'sonos',name:'Sonos',description:'控制你的 Sonos 音响系统',icon:'sonos',color:'#000000',url:'https://www.sonos.com/',category:'生活服务'},
  {id:'hevy',name:'Hevy',description:'健身计划、训练记录与洞察',icon:'dumbbell',color:'#18181B',url:'https://www.hevyapp.com/',category:'生活服务'},
];
